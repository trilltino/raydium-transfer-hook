//! The CPMM flow.

use raydium_adapters::swap::{cpmm_swap_instruction, cpmm_swap_output_instruction};
use solana_sdk::{instruction::Instruction, signature::Signer};

use super::{recorder::Recorder, support::*, swaps::*, world::*, FlowInputs};
use crate::{
    chain::{Chain, DriverError, Result},
    cpmm::{Cpmm, CpmmPool},
    env::Evidence,
    token,
};

const CPMM_SEED_AMOUNT: u64 = 1_000_000;
/// The AmmConfig index of the first extra pool; the next ones follow. Clear of the indices the liquidity
/// checks use.
const EXTRA_POOL_CONFIG_BASE: u16 = 10;

pub(super) struct CpmmSwaps<'a> {
    pub(super) kit: &'a SwapKit<'a>,
    pub(super) cpmm: Cpmm,
    pub(super) pool: CpmmPool,
    pub(super) world: &'a World,
}

impl SwapBuilder for CpmmSwaps<'_> {
    async fn build<C: Chain>(
        &self,
        chain: &C,
        mint0_in: bool,
        amount_in: u64,
        expected_out: u64,
    ) -> Result<Instruction> {
        let payer = chain.payer().pubkey();
        let (in_account, out_account) = if mint0_in {
            (self.world.trader[0].pubkey(), self.world.trader[1].pubkey())
        } else {
            (self.world.trader[1].pubkey(), self.world.trader[0].pubkey())
        };
        let accounts =
            self.cpmm
                .swap_accounts(payer, &self.pool, mint0_in, in_account, out_account);
        let (input_leg, output_leg) = legs(
            chain,
            self.kit,
            &accounts.input_token_mint,
            &accounts.output_token_mint,
            in_account,
            out_account,
            accounts.input_vault,
            accounts.output_vault,
            amount_in,
            expected_out,
        )
        .await?;
        cpmm_swap_instruction(
            &self.cpmm,
            &self.pool,
            payer,
            mint0_in,
            in_account,
            out_account,
            amount_in,
            1,
            &input_leg,
            &output_leg,
        )
        .map_err(|e| DriverError::new(format!("framing the CPMM swap failed: {e:?}")))
    }

    async fn build_exact_output<C: Chain>(
        &self,
        chain: &C,
        mint0_in: bool,
        max_amount_in: u64,
        amount_out: u64,
    ) -> Result<Instruction> {
        let payer = chain.payer().pubkey();
        let (in_account, out_account) = if mint0_in {
            (self.world.trader[0].pubkey(), self.world.trader[1].pubkey())
        } else {
            (self.world.trader[1].pubkey(), self.world.trader[0].pubkey())
        };
        let accounts =
            self.cpmm
                .swap_accounts(payer, &self.pool, mint0_in, in_account, out_account);
        let (input_leg, output_leg) = legs(
            chain,
            self.kit,
            &accounts.input_token_mint,
            &accounts.output_token_mint,
            in_account,
            out_account,
            accounts.input_vault,
            accounts.output_vault,
            max_amount_in,
            amount_out,
        )
        .await?;
        cpmm_swap_output_instruction(
            &self.cpmm,
            &self.pool,
            payer,
            mint0_in,
            in_account,
            out_account,
            max_amount_in,
            amount_out,
            &input_leg,
            &output_leg,
        )
        .map_err(|e| DriverError::new(format!("framing the CPMM exact-output swap failed: {e:?}")))
    }
}

/// Run the CPMM flow, and also return what it left behind (see [`crate::session::Session`]).
pub async fn run_cpmm_session<C: Chain>(
    chain: &mut C,
    inputs: &FlowInputs<'_>,
) -> Result<(Vec<Evidence>, crate::session::Session)> {
    inputs.env.require_hook_aware()?;
    let mut rec = Recorder::new("cpmm");
    let payer = chain.payer().pubkey();
    let seed_amount = inputs
        .ui_fixture
        .map_or(CPMM_SEED_AMOUNT, |fixture| fixture.seed_amount);
    let cpmm = Cpmm {
        program_id: inputs.env.cpmm_program()?,
        fee_receiver: inputs.env.cpmm_fee_receiver_key()?,
    };
    let admin = inputs.env.admin_key()?;

    // 1. Admin setup, only what is missing.
    let amm_config = cpmm.amm_config(0);
    if chain.account(&amm_config).await?.is_none() {
        require(
            payer == admin,
            format!(
                "AmmConfig {amm_config} is missing and the payer {payer} is not the admin {admin}"
            ),
        )?;
        send_step(
            chain,
            &mut rec,
            "create CPMM AmmConfig (admin instruction)",
            vec![cpmm.create_amm_config_instruction(&admin, 0, 2_500, 120_000, 0, 0, 0)],
            &[],
        )
        .await?;
    }
    if chain.account(&cpmm.fee_receiver).await?.is_none() {
        let keypair = inputs.fee_receiver_keypair.ok_or_else(|| {
            DriverError::new(format!(
                "the pool-creation fee receiver {} does not exist and no keypair for it was given",
                cpmm.fee_receiver
            ))
        })?;
        require(
            keypair.pubkey() == cpmm.fee_receiver,
            "the fee-receiver keypair does not match the environment's fee receiver",
        )?;
        send_step(
            chain,
            &mut rec,
            "create the wrapped-SOL pool-creation fee receiver",
            token::create_wsol_account_instructions(&payer, keypair, &admin),
            &[keypair],
        )
        .await?;
    }

    // 2. Mints and accounts, then the support-mint record for the hooked mint.
    let world = create_world(chain, &mut rec, inputs.world_options()).await?;
    require(
        payer == admin,
        "support mints can only be created by the admin",
    )?;
    let mut hooked_mints = vec![world.hooked.pubkey()];
    if inputs.second_hook.is_some() {
        hooked_mints.push(world.quote.pubkey());
    }
    let pool = cpmm.pool(amm_config, world.hooked.pubkey(), world.quote.pubkey());
    let unapproved_pool = cpmm.initialize_instruction(
        &payer,
        &pool,
        &world.provider[0].pubkey(),
        &world.provider[1].pubkey(),
        seed_amount,
        seed_amount,
        0,
        &[],
    );
    approve_hooked_mints(
        chain,
        &mut rec,
        inputs.env,
        crate::approval::Amm::Cpmm,
        "register a hooked mint with CPMM (admin instruction)",
        &hooked_mints,
        unapproved_pool,
    )
    .await?;

    // 3. Pool.
    send_step(
        chain,
        &mut rec,
        "create a real CPMM pool (initialize) with Token-2022 liquidity",
        with_budget(vec![cpmm.initialize_instruction(
            &payer,
            &pool,
            &world.provider[0].pubkey(),
            &world.provider[1].pubkey(),
            seed_amount,
            seed_amount,
            0,
            &hooked_mints
                .iter()
                .map(|mint| cpmm.support_mint(mint))
                .collect::<Vec<_>>(),
        )]),
        &[],
    )
    .await?;
    // A transfer fee is withheld from the deposit, so the vaults then hold slightly less.
    let seeded = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    let exact = inputs.transfer_fee_bps == 0;
    require(
        seeded.0 > 0
            && seeded.1 > 0
            && seeded.0 <= seed_amount
            && seeded.1 <= seed_amount
            && (!exact || (seeded.0 == seed_amount && seeded.1 == seed_amount)),
        format!("the pool vaults must hold the seeded liquidity, found {seeded:?}"),
    )?;
    // More pools of the same hooked mint, when asked for: each from its own AmmConfig, seeded like the first.
    let extra_pools = create_extra_pools(
        chain,
        &mut rec,
        &cpmm,
        &world,
        &hooked_mints,
        admin,
        seed_amount,
        inputs.extra_pools,
    )
    .await?;
    // CPMM opens a new pool one second after creation.
    chain.advance_time(5).await?;

    // A UI fixture's wallet is funded before the hook goes on: some hooks (holder-rewards) give up the
    // mint authority when they are enabled, and minting is not a transfer, so the hook is not involved.
    let wallet_accounts = match inputs.ui_fixture {
        Some(fixture) => Some(fund_ui_wallet(chain, &mut rec, &fixture, &world).await?),
        None => None,
    };

    // 4. Hook on, 5-6. Swap checks.
    let now = chain_time(chain).await?;
    let kit = SwapKit {
        hooks: hook_entries(
            inputs,
            &world,
            payer,
            pool.authority,
            [pool.vault_0, pool.vault_1],
            &extra_pools
                .iter()
                .map(|extra| extra.vault_0)
                .collect::<Vec<_>>(),
            now,
        ),
    };
    for (index, entry) in kit.hooks.iter().enumerate() {
        let role = if index == 0 {
            "hooked mint"
        } else {
            "second hooked mint"
        };
        enable_hook(chain, &mut rec, entry.hook, &entry.ctx, role).await?;
    }
    if let Some(fixture) = inputs.ui_fixture {
        let wallet_accounts = wallet_accounts.expect("a UI fixture funds its wallet");
        for (label, instructions) in kit.primary().hook.fixture_steps(&kit.primary().ctx) {
            send_step(chain, &mut rec, &label, with_budget(instructions), &[]).await?;
        }
        let mut session = build_session("cpmm", &world, &kit);
        session.pool = Some(pool.pool_state.to_string());
        session.extra_pools = extra_pools
            .iter()
            .map(|extra| extra.pool_state.to_string())
            .collect();
        // For a UI fixture the accounts are the wallet's, not the payer's.
        session.accounts = wallet_accounts.map(|account| account.to_string());
        rec.push(
            "summary",
            None,
            format!(
                "program {} pool {} hooked mint {} quote mint {} wallet {}",
                cpmm.program_id,
                pool.pool_state,
                world.hooked.pubkey(),
                world.quote.pubkey(),
                fixture.wallet
            ),
        );
        return Ok((rec.evidence, session));
    }
    let builder = CpmmSwaps {
        kit: &kit,
        cpmm,
        pool,
        world: &world,
    };
    swap_checks(chain, &mut rec, &builder, &kit).await?;
    if inputs.exact_output {
        exact_output_checks(chain, &mut rec, &builder, &kit, exact).await?;
    }
    if inputs.liquidity {
        super::liquidity::cpmm_liquidity_checks(
            chain,
            &mut rec,
            &cpmm,
            &world,
            &kit,
            &hooked_mints,
            admin,
            exact,
        )
        .await?;
    }
    rec.push(
        "summary",
        None,
        format!(
            "program {} pool {} hooked mint {} quote mint {}",
            cpmm.program_id,
            pool.pool_state,
            world.hooked.pubkey(),
            world.quote.pubkey()
        ),
    );
    let mut session = build_session("cpmm", &world, &kit);
    session.pool = Some(pool.pool_state.to_string());
    session.extra_pools = extra_pools
        .iter()
        .map(|extra| extra.pool_state.to_string())
        .collect();
    Ok((rec.evidence, session))
}

/// Create `count` more pools of the hooked mint and the quote mint, each under its own AmmConfig (the
/// pool address is derived from the config, so one pair can have several pools) and seeded from the
/// same liquidity providers.
#[allow(clippy::too_many_arguments)]
async fn create_extra_pools<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    cpmm: &Cpmm,
    world: &World,
    hooked_mints: &[solana_sdk::pubkey::Pubkey],
    admin: solana_sdk::pubkey::Pubkey,
    seed_amount: u64,
    count: u16,
) -> Result<Vec<CpmmPool>> {
    let payer = chain.payer().pubkey();
    require(
        u64::from(count + 1) * seed_amount <= PROVIDER_FUNDS,
        format!(
            "{} pools of {seed_amount} need more than the providers hold ({PROVIDER_FUNDS}); lower --seed-amount",
            count + 1
        ),
    )?;
    let mut pools = Vec::new();
    for index in 0..count {
        let config = cpmm.amm_config(EXTRA_POOL_CONFIG_BASE + index);
        if chain.account(&config).await?.is_none() {
            send_step(
                chain,
                rec,
                "create an AmmConfig for another pool of the same mint (admin instruction)",
                vec![cpmm.create_amm_config_instruction(
                    &admin,
                    EXTRA_POOL_CONFIG_BASE + index,
                    2_500,
                    120_000,
                    0,
                    0,
                    0,
                )],
                &[],
            )
            .await?;
        }
        let pool = cpmm.pool(config, world.hooked.pubkey(), world.quote.pubkey());
        send_step(
            chain,
            rec,
            "create another CPMM pool of the same hooked mint",
            with_budget(vec![cpmm.initialize_instruction(
                &payer,
                &pool,
                &world.provider[0].pubkey(),
                &world.provider[1].pubkey(),
                seed_amount,
                seed_amount,
                0,
                &hooked_mints
                    .iter()
                    .map(|mint| cpmm.support_mint(mint))
                    .collect::<Vec<_>>(),
            )]),
            &[],
        )
        .await?;
        pools.push(pool);
    }
    Ok(pools)
}

/// Run the CPMM flow.
pub async fn run_cpmm<C: Chain>(chain: &mut C, inputs: &FlowInputs<'_>) -> Result<Vec<Evidence>> {
    run_cpmm_session(chain, inputs)
        .await
        .map(|(evidence, _)| evidence)
}

/// The wallet's associated Token-2022 account for `mint`, created idempotently by the payer.
fn associated_account_instruction(
    payer: &solana_sdk::pubkey::Pubkey,
    owner: &solana_sdk::pubkey::Pubkey,
    mint: &solana_sdk::pubkey::Pubkey,
) -> (solana_sdk::pubkey::Pubkey, Instruction) {
    use solana_sdk::instruction::AccountMeta;

    let address = Cpmm::associated_token_2022(owner, mint);
    let instruction = Instruction {
        program_id: raydium_adapters::cpmm::ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(address, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
        ],
        // CreateIdempotent
        data: vec![1],
    };
    (address, instruction)
}

/// Give the browser test's wallet SOL and funded associated token accounts for both mints.
pub(super) async fn fund_ui_wallet<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    fixture: &super::UiFixture,
    world: &World,
) -> Result<[solana_sdk::pubkey::Pubkey; 2]> {
    let payer = chain.payer().pubkey();
    let (hooked_account, create_hooked) =
        associated_account_instruction(&payer, &fixture.wallet, &world.hooked.pubkey());
    let (quote_account, create_quote) =
        associated_account_instruction(&payer, &fixture.wallet, &world.quote.pubkey());
    send_step(
        chain,
        rec,
        "create the wallet's token accounts and fund it",
        vec![
            solana_sdk::system_instruction::transfer(
                &payer,
                &fixture.wallet,
                fixture.wallet_lamports,
            ),
            create_hooked,
            create_quote,
            token::mint_to_instruction(
                &world.hooked.pubkey(),
                &hooked_account,
                &payer,
                fixture.wallet_hooked_amount,
            ),
            token::mint_to_instruction(
                &world.quote.pubkey(),
                &quote_account,
                &payer,
                fixture.wallet_quote_amount,
            ),
        ],
        &[],
    )
    .await?;
    Ok([hooked_account, quote_account])
}
