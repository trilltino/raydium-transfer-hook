//! The CPMM flow.

use solana_sdk::{instruction::Instruction, signature::Signer};
use transfer_hook_sdk::{build_cpmm_swap_base_input_v1, frame_cpmm_or_passthrough};

use super::{recorder::Recorder, support::*, swaps::*, world::*, FlowInputs};
use crate::{
    chain::{Chain, DriverError, Result},
    cpmm::{Cpmm, CpmmPool},
    env::Evidence,
    hooks::HookSetup,
    token,
};

const CPMM_SEED_AMOUNT: u64 = 1_000_000;

pub(super) struct CpmmSwaps<'a> {
    pub(super) hook: &'a dyn HookSetup,
    pub(super) cpmm: Cpmm,
    pub(super) pool: CpmmPool,
    pub(super) world: &'a World,
}

impl SwapBuilder for CpmmSwaps<'_> {
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
        let accounts =
            self.cpmm
                .swap_accounts(payer, &self.pool, hooked_in, in_account, out_account);
        let (input_leg, output_leg) = legs(
            chain,
            self.hook,
            hooked_in,
            &self.world.hooked.pubkey(),
            &accounts.input_token_mint,
            &accounts.output_token_mint,
            in_account,
            out_account,
            accounts.input_vault,
            accounts.output_vault,
            payer,
            self.pool.authority,
            amount_in,
            expected_out,
        )
        .await?;
        let mut instruction =
            build_cpmm_swap_base_input_v1(self.cpmm.program_id, &accounts, amount_in, 1);
        frame_cpmm_or_passthrough(&mut instruction, &input_leg, &output_leg)
            .map_err(|e| DriverError::new(format!("framing the CPMM swap failed: {e:?}")))?;
        Ok(instruction)
    }
}

pub async fn run_cpmm<C: Chain>(chain: &mut C, inputs: &FlowInputs<'_>) -> Result<Vec<Evidence>> {
    inputs.env.require_hook_aware()?;
    let mut rec = Recorder::new("cpmm");
    let payer = chain.payer().pubkey();
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
    let world = create_world(chain, &mut rec).await?;
    require(
        payer == admin,
        "support mints can only be created by the admin",
    )?;
    send_step(
        chain,
        &mut rec,
        "register the hooked mint with CPMM (admin instruction)",
        vec![cpmm.create_support_mint_instruction(&admin, &world.hooked.pubkey())],
        &[],
    )
    .await?;

    // 3. Pool.
    let pool = cpmm.pool(amm_config, world.hooked.pubkey(), world.quote.pubkey());
    send_step(
        chain,
        &mut rec,
        "create a real CPMM pool (initialize) with Token-2022 liquidity",
        with_budget(vec![cpmm.initialize_instruction(
            &payer,
            &pool,
            &world.provider[0].pubkey(),
            &world.provider[1].pubkey(),
            CPMM_SEED_AMOUNT,
            CPMM_SEED_AMOUNT,
            0,
            &[cpmm.support_mint(&world.hooked.pubkey())],
        )]),
        &[],
    )
    .await?;
    require(
        amount_of(chain, &pool.vault_0).await? == CPMM_SEED_AMOUNT
            && amount_of(chain, &pool.vault_1).await? == CPMM_SEED_AMOUNT,
        "the pool vaults must hold the seeded liquidity",
    )?;
    // CPMM opens a new pool one second after creation.
    chain.advance_time(5).await?;

    // 4. Hook on, 5-6. Swap checks.
    enable_hook(chain, &mut rec, inputs.hook, &world.hooked.pubkey()).await?;
    let kit = SwapKit {
        hook: inputs.hook,
        hooked_mint: world.hooked.pubkey(),
        trader: [world.trader[0].pubkey(), world.trader[1].pubkey()],
    };
    let builder = CpmmSwaps {
        hook: inputs.hook,
        cpmm,
        pool,
        world: &world,
    };
    swap_checks(chain, &mut rec, &builder, &kit).await?;
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
    Ok(rec.evidence)
}
