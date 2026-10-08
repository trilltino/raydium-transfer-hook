//! The CPMM operations that move two tokens, with the hook live: creating a pool, depositing,
//! withdrawing and collecting fees. Each is the hook-aware `_v2` instruction, framed with the
//! token_0 transfer's hook slice and then the token_1 transfer's, like the swaps.
//!
//! They run on a second pool (a second AmmConfig with fees that actually accrue), created here
//! with the hook already on both mints' transfers, so creation itself is one of the things checked.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Signer};
use transfer_hook_sdk::{frame_cpmm_pair_or_passthrough, CpmmPairOp, SplTransferLeg};

use super::{
    cpmm::CpmmSwaps,
    recorder::Recorder,
    support::*,
    swaps::{describe, expected_runs, pair_legs, SwapBuilder, SwapKit},
    world::World,
};
use crate::{
    chain::{Chain, DriverError, Result},
    cpmm::Cpmm,
    hooks::{Direction, RejectionPlan},
};

/// Index of the second AmmConfig, whose fees accrue to the protocol, the fund and the creator.
const FEE_CONFIG_INDEX: u16 = 1;
/// Index of the third AmmConfig, for the pool made with `initialize_with_permission`, the only kind
/// that accrues creator fees.
const CREATOR_FEE_CONFIG_INDEX: u16 = 2;
/// Tokens put into the second pool, and the size of the swaps run on it.
const POOL_SEED_AMOUNT: u64 = 400;
const POOL_SWAP_AMOUNT: u64 = 300;
/// LP tokens deposited and withdrawn; the pool is about 1:1, so about this many of each token.
const LP_AMOUNT: u64 = 100;

/// Frame `instruction` as `op`'s `_v2` for the two transfer legs, and say how many hooked legs it has.
async fn frame_pair<C: Chain>(
    chain: &C,
    kit: &SwapKit<'_>,
    op: CpmmPairOp,
    instruction: &mut Instruction,
    token_0: SplTransferLeg,
    token_1: SplTransferLeg,
) -> Result<()> {
    let (leg_0, leg_1) = pair_legs(chain, kit, token_0, token_1).await?;
    let framed = frame_cpmm_pair_or_passthrough(op, instruction, &leg_0, &leg_1)
        .map_err(|e| DriverError::new(format!("framing {} failed: {e:?}", op.name())))?;
    require(
        framed.is_some(),
        format!("{} was not framed although a leg is hooked", op.name()),
    )
}

/// Simulate, check every hook ran once per hooked leg, send, and return the signature.
async fn run_hooked<C: Chain>(
    chain: &mut C,
    kit: &SwapKit<'_>,
    label: &str,
    instruction: Instruction,
) -> Result<(String, String)> {
    let transaction = with_budget(vec![instruction]);
    let sim = chain.simulate(&transaction, &[]).await?;
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
        .send(&transaction, &[])
        .await
        .map_err(|e| DriverError::new(format!("{label}: failed: {e}")))?;
    Ok((sent.signature, describe(&sim)))
}

/// One swap each way on the second pool, so that every kind of fee has something to collect.
async fn accrue_fees<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    swaps: &CpmmSwaps<'_>,
    kit: &SwapKit<'_>,
) -> Result<()> {
    for mint0_in in [true, false] {
        let instruction = swaps
            .build(chain, mint0_in, POOL_SWAP_AMOUNT, POOL_SWAP_AMOUNT - 2)
            .await?;
        let label = if mint0_in {
            "hooked token in"
        } else {
            "hooked token out"
        };
        let (signature, detail) = run_hooked(chain, kit, label, instruction).await?;
        rec.push(
            &format!("swap on the second pool ({label})"),
            Some(signature),
            detail,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn cpmm_liquidity_checks<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    cpmm: &Cpmm,
    world: &World,
    kit: &SwapKit<'_>,
    hooked_mints: &[Pubkey],
    admin: Pubkey,
    exact: bool,
) -> Result<()> {
    let payer = chain.payer().pubkey();
    require(
        payer == admin,
        "the liquidity checks collect fees, which the admin does so; the payer is not the admin",
    )?;
    let provider = [world.provider[0].pubkey(), world.provider[1].pubkey()];

    // A second AmmConfig whose fees accrue to all three recipients.
    let config = cpmm.amm_config(FEE_CONFIG_INDEX);
    if chain.account(&config).await?.is_none() {
        send_step(
            chain,
            rec,
            "create a second CPMM AmmConfig with protocol, fund and creator fees (admin instruction)",
            vec![cpmm.create_amm_config_instruction(
                &admin,
                FEE_CONFIG_INDEX,
                200_000,
                100_000,
                100_000,
                0,
                100_000,
            )],
            &[],
        )
        .await?;
    }
    let pool = cpmm.pool(config, world.hooked.pubkey(), world.quote.pubkey());
    let support_mints: Vec<Pubkey> = hooked_mints
        .iter()
        .map(|mint| cpmm.support_mint(mint))
        .collect();

    // 1. Pool creation with the hook already live: both seed transfers run the hook.
    let mut create = cpmm.initialize_instruction(
        &payer,
        &pool,
        &provider[0],
        &provider[1],
        POOL_SEED_AMOUNT,
        POOL_SEED_AMOUNT,
        0,
        &support_mints,
    );
    frame_pair(
        chain,
        kit,
        CpmmPairOp::Initialize,
        &mut create,
        SplTransferLeg {
            source: provider[0],
            mint: pool.mint_0,
            destination: pool.vault_0,
            authority: payer,
            amount: POOL_SEED_AMOUNT,
        },
        SplTransferLeg {
            source: provider[1],
            mint: pool.mint_1,
            destination: pool.vault_1,
            authority: payer,
            amount: POOL_SEED_AMOUNT,
        },
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "pool creation", create).await?;
    let seeded = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    require(
        seeded.0 > 0
            && seeded.1 > 0
            && seeded.0 <= POOL_SEED_AMOUNT
            && seeded.1 <= POOL_SEED_AMOUNT
            && (!exact || seeded == (POOL_SEED_AMOUNT, POOL_SEED_AMOUNT)),
        format!("the second pool's vaults must hold the seed liquidity, found {seeded:?}"),
    )?;
    rec.push(
        "hooked pool creation (initialize_v2)",
        Some(signature),
        format!(
            "the pool was created with the hook live; vaults {}/{}; {detail}",
            seeded.0, seeded.1
        ),
    );
    // A pool opens one second after it is created.
    chain.advance_time(5).await?;

    // 2. Swaps on it, both ways, so that every fee accrues in both tokens.
    let swaps = CpmmSwaps {
        kit,
        cpmm: *cpmm,
        pool,
        world,
    };
    accrue_fees(chain, rec, &swaps, kit).await?;

    // 3. Deposit, then withdraw, the same number of LP tokens.
    let lp_account = Cpmm::lp_token_account(&payer, &pool);
    let lp_before = amount_of(chain, &lp_account).await?;
    let vaults_before = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    let mut deposit = cpmm.deposit_instruction(
        &payer,
        &pool,
        &lp_account,
        &provider[0],
        &provider[1],
        LP_AMOUNT,
        u64::MAX / 2,
        u64::MAX / 2,
    );
    frame_pair(
        chain,
        kit,
        CpmmPairOp::Deposit,
        &mut deposit,
        SplTransferLeg {
            source: provider[0],
            mint: pool.mint_0,
            destination: pool.vault_0,
            authority: payer,
            amount: LP_AMOUNT,
        },
        SplTransferLeg {
            source: provider[1],
            mint: pool.mint_1,
            destination: pool.vault_1,
            authority: payer,
            amount: LP_AMOUNT,
        },
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "deposit", deposit).await?;
    let lp_after = amount_of(chain, &lp_account).await?;
    let vaults_after = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    require(
        lp_after == lp_before + LP_AMOUNT
            && vaults_after.0 > vaults_before.0
            && vaults_after.1 > vaults_before.1,
        format!("deposit: LP {lp_before}->{lp_after}, vaults {vaults_before:?}->{vaults_after:?}"),
    )?;
    rec.push(
        "hooked deposit (deposit_v2)",
        Some(signature),
        format!(
            "{LP_AMOUNT} LP tokens minted; vaults {vaults_before:?} -> {vaults_after:?}; {detail}"
        ),
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
        let mut over = cpmm.deposit_instruction(
            &payer,
            &pool,
            &lp_account,
            &provider[0],
            &provider[1],
            amount_in,
            u64::MAX / 2,
            u64::MAX / 2,
        );
        frame_pair(
            chain,
            kit,
            CpmmPairOp::Deposit,
            &mut over,
            SplTransferLeg {
                source: provider[0],
                mint: pool.mint_0,
                destination: pool.vault_0,
                authority: payer,
                amount: amount_in,
            },
            SplTransferLeg {
                source: provider[1],
                mint: pool.mint_1,
                destination: pool.vault_1,
                authority: payer,
                amount: amount_in,
            },
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
        let balances = (
            amount_of(chain, &pool.vault_0).await?,
            amount_of(chain, &pool.vault_1).await?,
            amount_of(chain, &lp_account).await?,
        );
        require(
            balances == (vaults_after.0, vaults_after.1, lp_after),
            "balances moved after a refused deposit",
        )?;
        rec.push(
            "hook refused deposit (over-amount), nothing changed",
            None,
            format!("rejected with {:#x}; {}", refusal.code, describe(&sim)),
        );
    }

    let mut withdraw = cpmm.withdraw_instruction(
        &payer,
        &pool,
        &lp_account,
        &provider[0],
        &provider[1],
        LP_AMOUNT,
        1,
        1,
    );
    frame_pair(
        chain,
        kit,
        CpmmPairOp::Withdraw,
        &mut withdraw,
        SplTransferLeg {
            source: pool.vault_0,
            mint: pool.mint_0,
            destination: provider[0],
            authority: pool.authority,
            amount: LP_AMOUNT,
        },
        SplTransferLeg {
            source: pool.vault_1,
            mint: pool.mint_1,
            destination: provider[1],
            authority: pool.authority,
            amount: LP_AMOUNT,
        },
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "withdraw", withdraw).await?;
    let lp_end = amount_of(chain, &lp_account).await?;
    let vaults_end = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    require(
        lp_end == lp_after - LP_AMOUNT
            && vaults_end.0 < vaults_after.0
            && vaults_end.1 < vaults_after.1,
        format!("withdraw: LP {lp_after}->{lp_end}, vaults {vaults_after:?}->{vaults_end:?}"),
    )?;
    rec.push(
        "hooked withdraw (withdraw_v2)",
        Some(signature),
        format!(
            "{LP_AMOUNT} LP tokens burned; vaults {vaults_after:?} -> {vaults_end:?}; {detail}"
        ),
    );

    // 4. Fee collection. The swaps above accrued protocol and fund fees in both tokens.
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
        let recipients_before = (
            amount_of(chain, &provider[0]).await?,
            amount_of(chain, &provider[1]).await?,
        );
        let mut collect = cpmm.collect_fee_instruction(
            fund,
            &payer,
            &pool,
            &provider[0],
            &provider[1],
            u64::MAX,
            u64::MAX,
        );
        let op = if fund {
            CpmmPairOp::CollectFundFee
        } else {
            CpmmPairOp::CollectProtocolFee
        };
        // The amounts only seed hook resolution; the program sends whatever has accrued.
        frame_pair(
            chain,
            kit,
            op,
            &mut collect,
            SplTransferLeg {
                source: pool.vault_0,
                mint: pool.mint_0,
                destination: provider[0],
                authority: pool.authority,
                amount: 1,
            },
            SplTransferLeg {
                source: pool.vault_1,
                mint: pool.mint_1,
                destination: provider[1],
                authority: pool.authority,
                amount: 1,
            },
        )
        .await?;
        let (signature, detail) = run_hooked(chain, kit, label, collect).await?;
        let recipients_after = (
            amount_of(chain, &provider[0]).await?,
            amount_of(chain, &provider[1]).await?,
        );
        require(
            recipients_after.0 > recipients_before.0 && recipients_after.1 > recipients_before.1,
            format!(
                "{label}: recipients {recipients_before:?}->{recipients_after:?}, both tokens' fees should have arrived"
            ),
        )?;
        rec.push(
            step,
            Some(signature),
            format!(
                "received {} and {}; {detail}",
                recipients_after.0 - recipients_before.0,
                recipients_after.1 - recipients_before.1
            ),
        );
    }

    creator_fee_checks(chain, rec, cpmm, world, kit, &support_mints, admin).await
}

/// Creator fees: a pool made with `initialize_with_permission_v2` (which needs an admin-created
/// permission record), swaps in both directions so fees accrue in both tokens, then the creator's fee
/// collected by `collect_creator_fee_v2` and, after more swaps, by
/// `collect_creator_fee_permissionless_v2`. Both transfers of each collection run the hook.
async fn creator_fee_checks<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    cpmm: &Cpmm,
    world: &World,
    kit: &SwapKit<'_>,
    support_mints: &[Pubkey],
    admin: Pubkey,
) -> Result<()> {
    let payer = chain.payer().pubkey();
    let provider = [world.provider[0].pubkey(), world.provider[1].pubkey()];

    let config = cpmm.amm_config(CREATOR_FEE_CONFIG_INDEX);
    if chain.account(&config).await?.is_none() {
        send_step(
            chain,
            rec,
            "create a third CPMM AmmConfig with a creator fee (admin instruction)",
            vec![cpmm.create_amm_config_instruction(
                &admin,
                CREATOR_FEE_CONFIG_INDEX,
                200_000,
                100_000,
                100_000,
                0,
                100_000,
            )],
            &[],
        )
        .await?;
    }
    let permission = cpmm.permission(&payer);
    if chain.account(&permission).await?.is_none() {
        send_step(
            chain,
            rec,
            "give the pool creator a permission record (admin instruction)",
            vec![cpmm.create_permission_instruction(&admin, &payer)],
            &[],
        )
        .await?;
    }

    // Pool creation with a permission record and the hook live.
    let pool = cpmm.pool(config, world.hooked.pubkey(), world.quote.pubkey());
    let mut create = cpmm.initialize_with_permission_instruction(
        &payer,
        &pool,
        &provider[0],
        &provider[1],
        POOL_SEED_AMOUNT,
        POOL_SEED_AMOUNT,
        0,
        0,
        support_mints,
    );
    frame_pair(
        chain,
        kit,
        CpmmPairOp::InitializeWithPermission,
        &mut create,
        SplTransferLeg {
            source: provider[0],
            mint: pool.mint_0,
            destination: pool.vault_0,
            authority: payer,
            amount: POOL_SEED_AMOUNT,
        },
        SplTransferLeg {
            source: provider[1],
            mint: pool.mint_1,
            destination: pool.vault_1,
            authority: payer,
            amount: POOL_SEED_AMOUNT,
        },
    )
    .await?;
    let (signature, detail) = run_hooked(chain, kit, "permissioned pool creation", create).await?;
    rec.push(
        "hooked pool creation with a permission record (initialize_with_permission_v2)",
        Some(signature),
        detail,
    );
    chain.advance_time(5).await?;

    let swaps = CpmmSwaps {
        kit,
        cpmm: *cpmm,
        pool,
        world,
    };
    let creator_accounts = [
        Cpmm::associated_token_2022(&payer, &pool.mint_0),
        Cpmm::associated_token_2022(&payer, &pool.mint_1),
    ];
    for (permissionless, step) in [
        (
            false,
            "hooked creator-fee collection (collect_creator_fee_v2)",
        ),
        (
            true,
            "hooked creator-fee collection by anyone (collect_creator_fee_permissionless_v2)",
        ),
    ] {
        accrue_fees(chain, rec, &swaps, kit).await?;
        let before = (
            amount_of(chain, &creator_accounts[0]).await.unwrap_or(0),
            amount_of(chain, &creator_accounts[1]).await.unwrap_or(0),
        );
        let mut collect =
            cpmm.collect_creator_fee_instruction(permissionless, &payer, &payer, &pool);
        let op = if permissionless {
            CpmmPairOp::CollectCreatorFeePermissionless
        } else {
            CpmmPairOp::CollectCreatorFee
        };
        // The amounts only seed hook resolution; the program sends whatever has accrued.
        frame_pair(
            chain,
            kit,
            op,
            &mut collect,
            SplTransferLeg {
                source: pool.vault_0,
                mint: pool.mint_0,
                destination: creator_accounts[0],
                authority: pool.authority,
                amount: 1,
            },
            SplTransferLeg {
                source: pool.vault_1,
                mint: pool.mint_1,
                destination: creator_accounts[1],
                authority: pool.authority,
                amount: 1,
            },
        )
        .await?;
        let (signature, detail) = run_hooked(chain, kit, "creator fee", collect).await?;
        let after = (
            amount_of(chain, &creator_accounts[0]).await?,
            amount_of(chain, &creator_accounts[1]).await?,
        );
        require(
            after.0 > before.0 && after.1 > before.1,
            format!("creator fee: creator balances {before:?}->{after:?}, both tokens' fees should have arrived"),
        )?;
        rec.push(
            step,
            Some(signature),
            format!(
                "creator received {} and {}; {detail}",
                after.0 - before.0,
                after.1 - before.1
            ),
        );
    }
    Ok(())
}
