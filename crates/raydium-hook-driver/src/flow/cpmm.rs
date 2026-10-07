//! The CPMM flow.

use raydium_adapters::swap::cpmm_swap_instruction;
use solana_sdk::{instruction::Instruction, signature::Signer};

use super::{recorder::Recorder, support::*, swaps::*, world::*, FlowInputs};
use crate::{
    chain::{Chain, DriverError, Result},
    cpmm::{Cpmm, CpmmPool},
    env::Evidence,
    token,
};

const CPMM_SEED_AMOUNT: u64 = 1_000_000;

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
}

/// Run the CPMM flow, and also return what it left behind (see [`crate::session::Session`]).
pub async fn run_cpmm_session<C: Chain>(
    chain: &mut C,
    inputs: &FlowInputs<'_>,
) -> Result<(Vec<Evidence>, crate::session::Session)> {
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
    let world = create_world(chain, &mut rec, inputs.world_options()).await?;
    require(
        payer == admin,
        "support mints can only be created by the admin",
    )?;
    let mut hooked_mints = vec![world.hooked.pubkey()];
    if inputs.second_hook.is_some() {
        hooked_mints.push(world.quote.pubkey());
    }
    for mint in &hooked_mints {
        send_step(
            chain,
            &mut rec,
            "register a hooked mint with CPMM (admin instruction)",
            vec![cpmm.create_support_mint_instruction(&admin, mint)],
            &[],
        )
        .await?;
    }

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
            && seeded.0 <= CPMM_SEED_AMOUNT
            && seeded.1 <= CPMM_SEED_AMOUNT
            && (!exact || (seeded.0 == CPMM_SEED_AMOUNT && seeded.1 == CPMM_SEED_AMOUNT)),
        format!("the pool vaults must hold the seeded liquidity, found {seeded:?}"),
    )?;
    // CPMM opens a new pool one second after creation.
    chain.advance_time(5).await?;

    // 4. Hook on, 5-6. Swap checks.
    let now = chain_time(chain).await?;
    let kit = SwapKit {
        hooks: hook_entries(
            inputs,
            &world,
            payer,
            pool.authority,
            [pool.vault_0, pool.vault_1],
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
    let builder = CpmmSwaps {
        kit: &kit,
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
    let session = build_session("cpmm", &world, &kit);
    Ok((rec.evidence, session))
}

/// Run the CPMM flow.
pub async fn run_cpmm<C: Chain>(chain: &mut C, inputs: &FlowInputs<'_>) -> Result<Vec<Evidence>> {
    run_cpmm_session(chain, inputs)
        .await
        .map(|(evidence, _)| evidence)
}
