//! The CLMM flow.

use raydium_adapters::swap::clmm_swap_instruction;
use solana_sdk::{
    instruction::Instruction,
    signature::{Keypair, Signer},
};

use super::{recorder::Recorder, support::*, swaps::*, world::*, FlowInputs};
use crate::{
    chain::{Chain, DriverError, Result},
    clmm::{Clmm, ClmmPool},
    env::Evidence,
};

const CLMM_LIQUIDITY: u128 = 100_000_000_000;

pub(super) struct ClmmSwaps<'a> {
    pub(super) kit: &'a SwapKit<'a>,
    pub(super) clmm: Clmm,
    pub(super) pool: ClmmPool,
    pub(super) world: &'a World,
}

impl SwapBuilder for ClmmSwaps<'_> {
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
        let (input_vault, output_vault, input_mint, output_mint) = if mint0_in {
            (
                self.pool.vault_0,
                self.pool.vault_1,
                self.pool.mint_0,
                self.pool.mint_1,
            )
        } else {
            (
                self.pool.vault_1,
                self.pool.vault_0,
                self.pool.mint_1,
                self.pool.mint_0,
            )
        };
        // In CLMM the pool-state PDA owns both vaults and signs the output transfer.
        let (input_leg, output_leg) = legs(
            chain,
            self.kit,
            &input_mint,
            &output_mint,
            in_account,
            out_account,
            input_vault,
            output_vault,
            amount_in,
            expected_out,
        )
        .await?;
        // zero_for_one (mint_0 in) walks A(0) then A(-600). After that swap the tick is -1, so the
        // reverse swap starts in A(-600) then A(0).
        let ticks = if mint0_in {
            self.pool.tick_arrays
        } else {
            [self.pool.tick_arrays[1], self.pool.tick_arrays[0]]
        };
        clmm_swap_instruction(
            &self.clmm,
            &self.pool,
            payer,
            mint0_in,
            in_account,
            out_account,
            amount_in,
            1,
            &ticks,
            &input_leg,
            &output_leg,
        )
        .map_err(|e| DriverError::new(format!("framing the CLMM swap failed: {e:?}")))
    }
}

/// Run the CLMM flow, and also return what it left behind (see [`crate::session::Session`]).
pub async fn run_clmm_session<C: Chain>(
    chain: &mut C,
    inputs: &FlowInputs<'_>,
) -> Result<(Vec<Evidence>, crate::session::Session)> {
    inputs.env.require_hook_aware()?;
    let mut rec = Recorder::new("clmm");
    let payer = chain.payer().pubkey();
    let clmm = Clmm {
        program_id: inputs.env.clmm_program()?,
    };
    let admin = inputs.env.admin_key()?;

    let amm_config = clmm.amm_config(0);
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
            "create CLMM AmmConfig (admin instruction)",
            vec![clmm.create_amm_config_instruction(
                &admin,
                0,
                crate::clmm::TICK_SPACING,
                2_500,
                120_000,
                0,
            )],
            &[],
        )
        .await?;
    }

    let world = create_world(chain, &mut rec, inputs.world_options()).await?;
    require(
        payer == admin,
        "support mints can only be created by the admin",
    )?;
    let mut hooked_mints = vec![world.hooked.pubkey()];
    if inputs.second_hook.is_some() {
        hooked_mints.push(world.quote.pubkey());
    }
    let pool = clmm.pool(amm_config, world.hooked.pubkey(), world.quote.pubkey());
    let unapproved_pool = clmm.create_pool_instruction(&payer, &pool, 1u128 << 64, 0, &[]);
    approve_hooked_mints(
        chain,
        &mut rec,
        inputs.env,
        crate::approval::Amm::Clmm,
        "register a hooked mint with CLMM (admin instruction)",
        &hooked_mints,
        unapproved_pool,
    )
    .await?;
    send_step(
        chain,
        &mut rec,
        "create a real CLMM pool (create_pool) at price 1",
        with_budget(vec![clmm.create_pool_instruction(
            &payer,
            &pool,
            1u128 << 64,
            0,
            &hooked_mints
                .iter()
                .map(|mint| clmm.support_mint(mint))
                .collect::<Vec<_>>(),
        )]),
        &[],
    )
    .await?;

    let position_nft = Keypair::new();
    send_step(
        chain,
        &mut rec,
        "open a real CLMM liquidity position over ticks [-300, 300] (creates both tick arrays)",
        with_budget(vec![clmm.open_position_instruction(
            &payer,
            &payer,
            &position_nft.pubkey(),
            &pool,
            &world.provider[0].pubkey(),
            &world.provider[1].pubkey(),
            CLMM_LIQUIDITY,
            PROVIDER_FUNDS,
            PROVIDER_FUNDS,
        )]),
        &[&position_nft],
    )
    .await?;
    let seeded = (
        amount_of(chain, &pool.vault_0).await?,
        amount_of(chain, &pool.vault_1).await?,
    );
    require(
        seeded.0 > 0 && seeded.1 > 0,
        "the position must fund both pool vaults",
    )?;
    rec.push(
        "vaults seeded by the position",
        None,
        format!("vault_0 {} vault_1 {}", seeded.0, seeded.1),
    );

    // A UI fixture's wallet is funded before the hook goes on: some hooks (holder-rewards) give up the
    // mint authority when they are enabled, and minting is not a transfer, so the hook is not involved.
    let wallet_accounts = match inputs.ui_fixture {
        Some(fixture) => {
            Some(super::cpmm::fund_ui_wallet(chain, &mut rec, &fixture, &world).await?)
        }
        None => None,
    };

    let now = chain_time(chain).await?;
    let kit = SwapKit {
        hooks: hook_entries(
            inputs,
            &world,
            payer,
            // In CLMM the pool-state PDA owns both vaults.
            pool.pool_state,
            [pool.vault_0, pool.vault_1],
            &[],
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
    if inputs.ui_fixture.is_some() {
        let wallet_accounts = wallet_accounts.expect("a UI fixture funds its wallet");
        for (label, instructions) in kit.primary().hook.fixture_steps(&kit.primary().ctx) {
            send_step(chain, &mut rec, &label, with_budget(instructions), &[]).await?;
        }
        let mut session = build_session("clmm", &world, &kit);
        session.pool = Some(pool.pool_state.to_string());
        // For a UI fixture the accounts are the wallet's, not the payer's.
        session.accounts = wallet_accounts.map(|account| account.to_string());
        return Ok((rec.evidence, session));
    }
    let builder = ClmmSwaps {
        kit: &kit,
        clmm,
        pool,
        world: &world,
    };
    swap_checks(chain, &mut rec, &builder, &kit).await?;
    if inputs.liquidity {
        super::clmm_liquidity::clmm_liquidity_checks(
            chain,
            &mut rec,
            &clmm,
            &world,
            inputs,
            &hooked_mints,
            admin,
        )
        .await?;
    }
    rec.push(
        "summary",
        None,
        format!(
            "program {} pool {} hooked mint {} quote mint {}",
            clmm.program_id,
            pool.pool_state,
            world.hooked.pubkey(),
            world.quote.pubkey()
        ),
    );
    let mut session = build_session("clmm", &world, &kit);
    session.pool = Some(pool.pool_state.to_string());
    Ok((rec.evidence, session))
}

/// Run the CLMM flow.
pub async fn run_clmm<C: Chain>(chain: &mut C, inputs: &FlowInputs<'_>) -> Result<Vec<Evidence>> {
    run_clmm_session(chain, inputs)
        .await
        .map(|(evidence, _)| evidence)
}
