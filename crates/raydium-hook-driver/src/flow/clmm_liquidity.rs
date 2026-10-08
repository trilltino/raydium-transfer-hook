//! The CLMM operations that move two tokens, with the hook live: opening a position, adding and
//! removing liquidity, collecting a position's fees, and the admin's protocol and fund fee
//! collections. Each is the hook-aware instruction (`*_v3`, `collect_*_v2`), framed with the
//! token_0 transfer's hook slice and then the token_1 transfer's, as the swaps are.
//!
//! They run on a second pool with an AmmConfig whose fees are large enough to leave something to
//! collect, created with the hook already on both mints, so a position opened with the hook live is
//! one of the things checked. Rewards are not covered: a reward mint with a Transfer Hook is not
//! supported.

use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use transfer_hook_sdk::{frame_clmm_liquidity_v3, ClmmLiquidityOp, SplTransferLeg};

use super::{
    clmm::ClmmSwaps,
    recorder::Recorder,
    support::*,
    swaps::{describe, expected_runs, hook_entries, pair_legs, SwapBuilder, SwapKit},
    world::World,
    FlowInputs,
};
use crate::{
    chain::{Chain, DriverError, Result},
    clmm::{Clmm, ClmmPool, ClmmPosition},
    hooks::{Direction, RejectionPlan},
};

/// Index of the second AmmConfig: a 10% trade fee, of which half goes to the protocol and 30% to the
/// fund, so every kind of fee has something to collect after a few small swaps.
const FEE_CONFIG_INDEX: u16 = 1;
const TRADE_FEE: u32 = 100_000;
const PROTOCOL_FEE: u32 = 500_000;
const FUND_FEE: u32 = 300_000;
/// Liquidity of the opened position and of the increase. At price 1 over ticks [-300, 300] a unit of
/// liquidity is worth about 0.0149 of each token, so these deposit about 300 and 150 of each, within
/// what every hook in this repository allows per transfer.
const OPEN_LIQUIDITY: u128 = 20_000;
const INCREASE_LIQUIDITY: u128 = 10_000;
/// Size of the swaps that accrue fees, and how many go each way.
const SWAP_AMOUNT: u64 = 100;
const SWAPS_EACH_WAY: usize = 3;

/// Frame `instruction` as `op`'s hook-aware version for the two transfer legs.
async fn frame<C: Chain>(
    chain: &C,
    kit: &SwapKit<'_>,
    op: ClmmLiquidityOp,
    instruction: &mut Instruction,
    token_0: SplTransferLeg,
    token_1: SplTransferLeg,
) -> Result<()> {
    let (leg_0, leg_1) = pair_legs(chain, kit, token_0, token_1).await?;
    frame_clmm_liquidity_v3(op, instruction, &leg_0, &leg_1)
        .map(|_| ())
        .map_err(|e| DriverError::new(format!("framing {} failed: {e:?}", op.name())))
}

/// Simulate, check every hook ran once per hooked leg, send, and return the signature and a summary.
async fn run_hooked<C: Chain>(
    chain: &mut C,
    kit: &SwapKit<'_>,
    label: &str,
    instruction: Instruction,
    signers: &[&Keypair],
) -> Result<(String, String)> {
    let transaction = with_budget(vec![instruction]);
    let sim = chain.simulate(&transaction, signers).await?;
    require(
        sim.succeeded,
        format!("{label}: simulation failed: {:?} {:?}", sim.error, sim.logs),
    )?;
    for (program, expected) in expected_runs(kit) {
        require(
            sim.invocations_of(&program) == expected,
            format!(
                "{label}: hook {program} must run {expected} time(s) (once per hooked transfer), ran {}",
                sim.invocations_of(&program)
            ),
        )?;
    }
    let sent = chain
        .send(&transaction, signers)
        .await
        .map_err(|e| DriverError::new(format!("{label}: failed: {e}")))?;
    Ok((sent.signature, describe(&sim)))
}

/// A transfer into the pool: the payer's account to the vault, signed by the payer.
fn deposit_legs(
    pool: &ClmmPool,
    payer: Pubkey,
    accounts: [Pubkey; 2],
    amount: u64,
) -> (SplTransferLeg, SplTransferLeg) {
    (
        SplTransferLeg {
            source: accounts[0],
            mint: pool.mint_0,
            destination: pool.vault_0,
            authority: payer,
            amount,
        },
        SplTransferLeg {
            source: accounts[1],
            mint: pool.mint_1,
            destination: pool.vault_1,
            authority: payer,
            amount,
        },
    )
}

/// A transfer out of the pool: the vault to a recipient account, signed by the pool state.
fn withdrawal_legs(
    pool: &ClmmPool,
    recipients: [Pubkey; 2],
    amount: u64,
) -> (SplTransferLeg, SplTransferLeg) {
    (
        SplTransferLeg {
            source: pool.vault_0,
            mint: pool.mint_0,
            destination: recipients[0],
            authority: pool.pool_state,
            amount,
        },
        SplTransferLeg {
            source: pool.vault_1,
            mint: pool.mint_1,
            destination: recipients[1],
            authority: pool.pool_state,
            amount,
        },
    )
}

async fn balances<C: Chain>(chain: &mut C, accounts: &[Pubkey; 2]) -> Result<(u64, u64)> {
    Ok((
        amount_of(chain, &accounts[0]).await?,
        amount_of(chain, &accounts[1]).await?,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn clmm_liquidity_checks<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    clmm: &Clmm,
    world: &World,
    inputs: &FlowInputs<'_>,
    hooked_mints: &[Pubkey],
    admin: Pubkey,
) -> Result<()> {
    let payer = chain.payer().pubkey();
    require(
        payer == admin,
        "the CLMM liquidity checks collect protocol and fund fees, which the admin does; the payer is not the admin",
    )?;
    let provider = [world.provider[0].pubkey(), world.provider[1].pubkey()];

    // 1. A second AmmConfig with fees that leave something to collect, and a pool on it.
    let config = clmm.amm_config(FEE_CONFIG_INDEX);
    if chain.account(&config).await?.is_none() {
        send_step(
            chain,
            rec,
            "create a second CLMM AmmConfig with protocol and fund fees (admin instruction)",
            vec![clmm.create_amm_config_instruction(
                &admin,
                FEE_CONFIG_INDEX,
                crate::clmm::TICK_SPACING,
                TRADE_FEE,
                PROTOCOL_FEE,
                FUND_FEE,
            )],
            &[],
        )
        .await?;
    }
    let pool = clmm.pool(config, world.hooked.pubkey(), world.quote.pubkey());
    let support_mints: Vec<Pubkey> = hooked_mints
        .iter()
        .map(|mint| clmm.support_mint(mint))
        .collect();
    send_step(
        chain,
        rec,
        "create a second CLMM pool (create_pool) at price 1",
        with_budget(vec![clmm.create_pool_instruction(
            &payer,
            &pool,
            1u128 << 64,
            0,
            &support_mints,
        )]),
        &[],
    )
    .await?;

    // The hook's view of this pool: in CLMM the pool state owns the vaults and signs the output leg, so
    // its transfers differ from the first pool's.
    let now = chain_time(chain).await?;
    let kit = &SwapKit {
        hooks: hook_entries(
            inputs,
            world,
            payer,
            pool.pool_state,
            [pool.vault_0, pool.vault_1],
            now,
        ),
    };

    // 2. Open a position with the hook already live: both deposits run the hook.
    let nft = Keypair::new();
    let position: ClmmPosition = clmm.position(&payer, &nft.pubkey());
    let mut open = clmm.open_position_instruction(
        &payer,
        &payer,
        &nft.pubkey(),
        &pool,
        &provider[0],
        &provider[1],
        OPEN_LIQUIDITY,
        1_000_000,
        1_000_000,
    );
    let (leg_0, leg_1) = deposit_legs(&pool, payer, provider, 1);
    frame(
        chain,
        kit,
        ClmmLiquidityOp::OpenPositionWithToken22Nft,
        &mut open,
        leg_0,
        leg_1,
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "open position", open, &[&nft]).await?;
    let vaults = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    require(
        vaults.0 > 0 && vaults.1 > 0,
        format!("the opened position must fund both vaults, found {vaults:?}"),
    )?;
    rec.push(
        "hooked position opening (open_position_with_token22_nft_v3)",
        Some(signature),
        format!(
            "the position was opened with the hook live; vaults {}/{}; {detail}",
            vaults.0, vaults.1
        ),
    );

    // 3. Add liquidity.
    let provider_before = balances(chain, &provider).await?;
    let vaults_before = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    let mut increase = clmm.increase_liquidity_instruction(
        &pool,
        &position,
        &provider[0],
        &provider[1],
        INCREASE_LIQUIDITY,
        1_000_000,
        1_000_000,
    );
    let (leg_0, leg_1) = deposit_legs(&pool, payer, provider, 1);
    frame(
        chain,
        kit,
        ClmmLiquidityOp::IncreaseLiquidity,
        &mut increase,
        leg_0,
        leg_1,
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "increase liquidity", increase, &[]).await?;
    let vaults_after = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    let provider_after = balances(chain, &provider).await?;
    require(
        vaults_after.0 > vaults_before.0
            && vaults_after.1 > vaults_before.1
            && provider_after.0 < provider_before.0
            && provider_after.1 < provider_before.1,
        format!(
            "increase: vaults {vaults_before:?}->{vaults_after:?}, payer {provider_before:?}->{provider_after:?}"
        ),
    )?;
    rec.push(
        "hooked liquidity increase (increase_liquidity_v3)",
        Some(signature),
        format!("vaults {vaults_before:?} -> {vaults_after:?}; {detail}"),
    );

    // A deposit the hook refuses (if it declares an amount rule): nothing may move.
    let hook = kit.primary();
    let refusal = hook.hook.refusals().into_iter().find(|r| {
        r.direction == Direction::HookedIn && matches!(r.plan, RejectionPlan::OverAmount { .. })
    });
    if let Some(refusal) = refusal {
        let RejectionPlan::OverAmount { amount_in } = refusal.plan else {
            unreachable!("matched above")
        };
        // About 0.0149 of each token per unit of liquidity: this deposits comfortably more than the limit.
        let liquidity = u128::from(amount_in) * 100;
        let mut over = clmm.increase_liquidity_instruction(
            &pool,
            &position,
            &provider[0],
            &provider[1],
            liquidity,
            u64::MAX / 2,
            u64::MAX / 2,
        );
        let (leg_0, leg_1) = deposit_legs(&pool, payer, provider, amount_in);
        frame(
            chain,
            kit,
            ClmmLiquidityOp::IncreaseLiquidity,
            &mut over,
            leg_0,
            leg_1,
        )
        .await?;
        let transaction = with_budget(vec![over]);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            !sim.succeeded && sim.program_failed_with(&hook.hook.program_id(), refusal.code),
            format!(
                "an over-limit deposit must be refused by the hook with {:#x}: {:?}",
                refusal.code, sim.logs
            ),
        )?;
        require(
            (
                amount_of(chain, &pool.vault_0).await?,
                amount_of(chain, &pool.vault_1).await?,
                balances(chain, &provider).await?,
            ) == (vaults_after.0, vaults_after.1, provider_after),
            "balances moved after a refused deposit",
        )?;
        rec.push(
            "hook refused liquidity increase (over-amount), nothing changed",
            None,
            format!("rejected with {:#x}; {}", refusal.code, describe(&sim)),
        );
    }

    // 4. Swaps both ways, so that every kind of fee accrues in both tokens.
    let swaps = ClmmSwaps {
        kit,
        clmm: *clmm,
        pool,
        world,
    };
    for mint0_in in [true, false] {
        for _ in 0..SWAPS_EACH_WAY {
            let instruction = swaps.build(chain, mint0_in, SWAP_AMOUNT, 1).await?;
            let label = if mint0_in {
                "hooked token in"
            } else {
                "hooked token out"
            };
            let (signature, detail) = run_hooked(chain, kit, label, instruction, &[]).await?;
            rec.push(
                &format!("swap on the second CLMM pool ({label})"),
                Some(signature),
                detail,
            );
        }
    }

    // 5. The position's own fees: decreasing by zero collects them.
    let provider_before = balances(chain, &provider).await?;
    let mut collect =
        clmm.decrease_liquidity_instruction(&pool, &position, &provider[0], &provider[1], 0, 0, 0);
    let (leg_0, leg_1) = withdrawal_legs(&pool, provider, 1);
    frame(
        chain,
        kit,
        ClmmLiquidityOp::DecreaseLiquidity,
        &mut collect,
        leg_0,
        leg_1,
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "position fees", collect, &[]).await?;
    let provider_after = balances(chain, &provider).await?;
    require(
        provider_after.0 > provider_before.0 && provider_after.1 > provider_before.1,
        format!("position fees: recipients {provider_before:?}->{provider_after:?}, both tokens' fees should have arrived"),
    )?;
    rec.push(
        "hooked position-fee collection (decrease_liquidity_v3 with zero liquidity)",
        Some(signature),
        format!(
            "received {} and {}; {detail}",
            provider_after.0 - provider_before.0,
            provider_after.1 - provider_before.1
        ),
    );

    // 6. The admin's protocol and fund fees.
    for (fund, label, step) in [
        (
            false,
            "protocol fee",
            "hooked protocol-fee collection (collect_protocol_fee_v2)",
        ),
        (
            true,
            "fund fee",
            "hooked fund-fee collection (collect_fund_fee_v2)",
        ),
    ] {
        let before = balances(chain, &provider).await?;
        let mut collect = clmm.collect_fee_instruction(
            fund,
            &payer,
            &pool,
            &provider[0],
            &provider[1],
            u64::MAX,
            u64::MAX,
        );
        let op = if fund {
            ClmmLiquidityOp::CollectFundFee
        } else {
            ClmmLiquidityOp::CollectProtocolFee
        };
        // The amounts only seed hook resolution; the program sends whatever has accrued.
        let (leg_0, leg_1) = withdrawal_legs(&pool, provider, 1);
        frame(chain, kit, op, &mut collect, leg_0, leg_1).await?;
        let (signature, detail) = run_hooked(chain, kit, label, collect, &[]).await?;
        let after = balances(chain, &provider).await?;
        require(
            after.0 > before.0 && after.1 > before.1,
            format!(
                "{label}: recipients {before:?}->{after:?}, both tokens' fees should have arrived"
            ),
        )?;
        rec.push(
            step,
            Some(signature),
            format!(
                "received {} and {}; {detail}",
                after.0 - before.0,
                after.1 - before.1
            ),
        );
    }

    // 7. Take the whole position out.
    let before = balances(chain, &provider).await?;
    let vaults_before = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    let mut remove = clmm.decrease_liquidity_instruction(
        &pool,
        &position,
        &provider[0],
        &provider[1],
        OPEN_LIQUIDITY + INCREASE_LIQUIDITY,
        0,
        0,
    );
    let (leg_0, leg_1) = withdrawal_legs(&pool, provider, 1);
    frame(
        chain,
        kit,
        ClmmLiquidityOp::DecreaseLiquidity,
        &mut remove,
        leg_0,
        leg_1,
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "remove liquidity", remove, &[]).await?;
    let after = balances(chain, &provider).await?;
    let vaults_after = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    require(
        after.0 > before.0
            && after.1 > before.1
            && vaults_after.0 < vaults_before.0
            && vaults_after.1 < vaults_before.1,
        format!(
            "remove: recipients {before:?}->{after:?}, vaults {vaults_before:?}->{vaults_after:?}"
        ),
    )?;
    rec.push(
        "hooked liquidity removal (decrease_liquidity_v3)",
        Some(signature),
        format!("vaults {vaults_before:?} -> {vaults_after:?}; {detail}"),
    );
    Ok(())
}
