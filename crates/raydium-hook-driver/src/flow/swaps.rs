//! The swap checks both AMMs share: leg resolution, hooked swaps, refused swaps.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{LegHook, LegRole, SplTransferLeg};

use super::{recorder::Recorder, support::*, SWAP_AMOUNT};
use crate::{
    chain::{Chain, DriverError, Result, Simulation},
    hooks::{HookSetup, RejectionPlan},
};

/// What a flow needs to know to run the swap checks, independent of the AMM.
pub(super) struct SwapKit<'a> {
    pub(super) hook: &'a dyn HookSetup,
    pub(super) hooked_mint: Pubkey,
    /// Trader accounts: `[hooked, quote]`.
    pub(super) trader: [Pubkey; 2],
}

/// Build the framed swap instruction(s) for one direction. `hooked_in` is true when the hooked
/// token is the input.
pub(super) trait SwapBuilder {
    async fn build<C: Chain>(
        &self,
        chain: &C,
        hooked_in: bool,
        amount_in: u64,
        expected_out: u64,
    ) -> Result<Instruction>;
}

/// Resolve the two transfer legs of a swap independently. Only the hooked mint (`mint_0`) can
/// carry a hook; the quote leg is resolved too and must come back unhooked.
#[allow(clippy::too_many_arguments)]
pub(super) async fn legs<C: Chain>(
    chain: &C,
    hook: &dyn HookSetup,
    hooked_in: bool,
    hooked_mint: &Pubkey,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    input_account: Pubkey,
    output_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    payer: Pubkey,
    pool_authority: Pubkey,
    amount_in: u64,
    expected_out: u64,
) -> Result<(LegHook, LegHook)> {
    let policy_for = |mint: &Pubkey| -> (Option<Pubkey>, Vec<Pubkey>) {
        if mint == hooked_mint {
            (
                Some(hook.program_id()),
                hook.state_account(hooked_mint).into_iter().collect(),
            )
        } else {
            (None, Vec::new())
        }
    };
    let (input_program, input_writable) = policy_for(input_mint);
    let (output_program, output_writable) = policy_for(output_mint);
    let input = resolve(
        chain,
        LegRole::Input,
        SplTransferLeg {
            source: input_account,
            mint: *input_mint,
            destination: input_vault,
            authority: payer,
            amount: amount_in,
        },
        input_program,
        input_writable,
    )
    .await?;
    let output = resolve(
        chain,
        LegRole::Output,
        SplTransferLeg {
            source: output_vault,
            mint: *output_mint,
            destination: output_account,
            authority: pool_authority,
            amount: expected_out,
        },
        output_program,
        output_writable,
    )
    .await?;
    require(
        input.is_hooked() == hooked_in && output.is_hooked() != hooked_in,
        format!(
            "exactly the hooked leg must resolve a hook (input hooked: {}, output hooked: {})",
            input.is_hooked(),
            output.is_hooked()
        ),
    )?;
    Ok((input, output))
}

pub(super) fn describe(sim: &Simulation) -> String {
    format!(
        "simulation {}: {} log lines, {} compute units",
        if sim.succeeded { "ok" } else { "failed" },
        sim.logs.len(),
        sim.units_consumed
            .map(|u| u.to_string())
            .unwrap_or_else(|| "?".into())
    )
}

/// Steps 5 and 6, shared by both AMMs.
pub(super) async fn swap_checks<C: Chain, B: SwapBuilder>(
    chain: &mut C,
    rec: &mut Recorder,
    builder: &B,
    kit: &SwapKit<'_>,
) -> Result<()> {
    let hook_program = kit.hook.program_id();

    // Hooked token in, then hooked token out. Each: simulate, check the hook ran once, send.
    for hooked_in in [true, false] {
        let direction = if hooked_in {
            "hooked token in"
        } else {
            "hooked token out"
        };
        let before_hooked = amount_of(chain, &kit.trader[0]).await?;
        let before_quote = amount_of(chain, &kit.trader[1]).await?;
        let state_before = match kit.hook.state_account(&kit.hooked_mint) {
            Some(key) => Some((key, raw_data(chain, &key).await?)),
            None => None,
        };
        let instruction = builder
            .build(chain, hooked_in, SWAP_AMOUNT, SWAP_AMOUNT.saturating_sub(2))
            .await?;
        let transaction = with_budget(vec![instruction]);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            sim.succeeded,
            format!(
                "{direction}: swap simulation failed: {:?} {:?}",
                sim.error, sim.logs
            ),
        )?;
        require(
            sim.invocations_of(&hook_program) == 1,
            format!(
                "{direction}: the hook must run exactly once (the hooked leg), ran {} times",
                sim.invocations_of(&hook_program)
            ),
        )?;
        let sent = chain
            .send(&transaction, &[])
            .await
            .map_err(|e| DriverError::new(format!("{direction}: swap failed: {e}")))?;
        let after_hooked = amount_of(chain, &kit.trader[0]).await?;
        let after_quote = amount_of(chain, &kit.trader[1]).await?;
        if hooked_in {
            require(
                after_hooked == before_hooked - SWAP_AMOUNT && after_quote > before_quote,
                format!("{direction}: unexpected balances {before_hooked}/{before_quote} -> {after_hooked}/{after_quote}"),
            )?;
        } else {
            require(
                after_quote == before_quote - SWAP_AMOUNT && after_hooked > before_hooked,
                format!("{direction}: unexpected balances {before_hooked}/{before_quote} -> {after_hooked}/{after_quote}"),
            )?;
        }
        let mut detail = format!(
            "hook ran once; hooked {before_hooked}->{after_hooked}, quote {before_quote}->{after_quote}; {}",
            describe(&sim)
        );
        if let Some((key, before)) = state_before {
            let after = raw_data(chain, &key).await?;
            require(
                after != before,
                format!("{direction}: the hook's state account {key} did not change"),
            )?;
            detail.push_str("; hook state account changed");
        }
        rec.push(
            &format!("hooked swap ({direction})"),
            Some(sent.signature),
            detail,
        );
    }

    // A swap the hook refuses, with the hooked token in and then out.
    for hooked_in in [true, false] {
        let direction = if hooked_in {
            "hooked token in"
        } else {
            "hooked token out"
        };
        let balances_before = (
            amount_of(chain, &kit.trader[0]).await?,
            amount_of(chain, &kit.trader[1]).await?,
        );
        let (amount_in, copies) = match kit.hook.rejection_plan() {
            RejectionPlan::OverAmount { amount_in } => (amount_in, 1),
            RejectionPlan::RepeatInOneTransaction { times } => (SWAP_AMOUNT, times),
        };
        let mut instructions = Vec::new();
        for _ in 0..copies {
            instructions.push(
                builder
                    .build(chain, hooked_in, amount_in, amount_in.saturating_sub(2))
                    .await?,
            );
        }
        let transaction = with_budget(instructions);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            !sim.succeeded,
            format!("{direction}: the hook was supposed to refuse this swap but it succeeded"),
        )?;
        require(
            sim.program_failed_with(&hook_program, kit.hook.rejection_code()),
            format!(
                "{direction}: the failure did not come from the hook with code {:#x}: {:?}",
                kit.hook.rejection_code(),
                sim.logs
            ),
        )?;
        let error = match chain.send(&transaction, &[]).await {
            Ok(_) => {
                return Err(DriverError::new(format!(
                    "{direction}: the refused swap was accepted"
                )))
            }
            Err(error) => error,
        };
        require(
            error.custom_code == Some(kit.hook.rejection_code()),
            format!("{direction}: wrong error from the refused swap: {error}"),
        )?;
        let balances_after = (
            amount_of(chain, &kit.trader[0]).await?,
            amount_of(chain, &kit.trader[1]).await?,
        );
        require(
            balances_before == balances_after,
            format!("{direction}: balances changed after a refused swap: {balances_before:?} -> {balances_after:?}"),
        )?;
        rec.push(
            &format!("hook refused swap ({direction}), nothing changed"),
            None,
            format!(
                "hook {} rejected with {:#x} after {} hook run(s); {} swap instruction(s) in the transaction rolled back; {}",
                hook_program,
                kit.hook.rejection_code(),
                sim.invocations_of(&hook_program),
                copies,
                describe(&sim)
            ),
        );
    }
    Ok(())
}
