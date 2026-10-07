//! The CLMM flow.

use solana_sdk::{
    instruction::Instruction,
    signature::{Keypair, Signer},
};
use transfer_hook_sdk::{
    build_clmm_swap_v2, frame_clmm_or_passthrough, ClmmSwapAccounts, ClmmSwapArgs,
};

use super::{recorder::Recorder, support::*, swaps::*, world::*, FlowInputs};
use crate::{
    chain::{Chain, DriverError, Result},
    clmm::{Clmm, ClmmPool, MEMO_PROGRAM_ID},
    env::Evidence,
    hooks::HookSetup,
};

const CLMM_LIQUIDITY: u128 = 100_000_000_000;

pub(super) struct ClmmSwaps<'a> {
    pub(super) hook: &'a dyn HookSetup,
    pub(super) clmm: Clmm,
    pub(super) pool: ClmmPool,
    pub(super) world: &'a World,
}

impl SwapBuilder for ClmmSwaps<'_> {
    async fn build<C: Chain>(
        &self,
        chain: &C,
        hooked_in: bool,
        amount_in: u64,
        expected_out: u64,
    ) -> Result<Instruction> {
        let payer = chain.payer().pubkey();
        let (in_account, out_account) = if hooked_in {
            (self.world.trader[0].pubkey(), self.world.trader[1].pubkey())
        } else {
            (self.world.trader[1].pubkey(), self.world.trader[0].pubkey())
        };
        let (input_vault, output_vault, input_mint, output_mint) = if hooked_in {
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
            self.hook,
            hooked_in,
            &self.world.hooked.pubkey(),
            &input_mint,
            &output_mint,
            in_account,
            out_account,
            input_vault,
            output_vault,
            payer,
            self.pool.pool_state,
            amount_in,
            expected_out,
        )
        .await?;
        let accounts = ClmmSwapAccounts {
            payer,
            amm_config: self.pool.amm_config,
            pool_state: self.pool.pool_state,
            input_token_account: in_account,
            output_token_account: out_account,
            input_vault,
            output_vault,
            observation_state: self.pool.observation,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
            memo_program: MEMO_PROGRAM_ID,
            input_vault_mint: input_mint,
            output_vault_mint: output_mint,
        };
        // zero_for_one (hooked token in, it is mint_0) walks A(0) then A(-600). After that swap
        // the tick is -1, so the reverse swap starts in A(-600) then A(0).
        let ticks = if hooked_in {
            self.pool.tick_arrays
        } else {
            [self.pool.tick_arrays[1], self.pool.tick_arrays[0]]
        };
        let mut instruction = build_clmm_swap_v2(
            self.clmm.program_id,
            &accounts,
            &ticks,
            None,
            ClmmSwapArgs {
                amount: amount_in,
                other_amount_threshold: 1,
                sqrt_price_limit_x64: 0,
                is_base_input: true,
            },
        );
        frame_clmm_or_passthrough(&mut instruction, 2, 0, &input_leg, &output_leg)
            .map_err(|e| DriverError::new(format!("framing the CLMM swap failed: {e:?}")))?;
        Ok(instruction)
    }
}

pub async fn run_clmm<C: Chain>(chain: &mut C, inputs: &FlowInputs<'_>) -> Result<Vec<Evidence>> {
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

    let world = create_world(chain, &mut rec).await?;
    require(
        payer == admin,
        "support mints can only be created by the admin",
    )?;
    send_step(
        chain,
        &mut rec,
        "register the hooked mint with CLMM (admin instruction)",
        vec![clmm.create_support_mint_instruction(&admin, &world.hooked.pubkey())],
        &[],
    )
    .await?;

    let pool = clmm.pool(amm_config, world.hooked.pubkey(), world.quote.pubkey());
    send_step(
        chain,
        &mut rec,
        "create a real CLMM pool (create_pool) at price 1",
        with_budget(vec![clmm.create_pool_instruction(
            &payer,
            &pool,
            1u128 << 64,
            0,
            &[clmm.support_mint(&world.hooked.pubkey())],
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

    enable_hook(chain, &mut rec, inputs.hook, &world.hooked.pubkey()).await?;
    let kit = SwapKit {
        hook: inputs.hook,
        hooked_mint: world.hooked.pubkey(),
        trader: [world.trader[0].pubkey(), world.trader[1].pubkey()],
    };
    let builder = ClmmSwaps {
        hook: inputs.hook,
        clmm,
        pool,
        world: &world,
    };
    swap_checks(chain, &mut rec, &builder, &kit).await?;
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
    Ok(rec.evidence)
}
