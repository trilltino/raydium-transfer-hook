//! The swap checks both AMMs share: leg resolution, hooked swaps, refused swaps, follow-ups.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair};
use transfer_hook_sdk::{LegHook, LegRole, SplTransferLeg};

use super::{recorder::Recorder, support::*, SWAP_AMOUNT};
use crate::{
    chain::{Chain, DriverError, Result, Simulation},
    hooks::{Direction, FollowUp, HookContext, HookSetup, RejectionPlan},
};

/// What a flow needs to know to run the swap checks, independent of the AMM.
pub(super) struct SwapKit<'a> {
    pub(super) hook: &'a dyn HookSetup,
    pub(super) ctx: HookContext,
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
    ctx: &HookContext,
    hooked_in: bool,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    input_account: Pubkey,
    output_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    amount_in: u64,
    expected_out: u64,
) -> Result<(LegHook, LegHook)> {
    let input_leg = SplTransferLeg {
        source: input_account,
        mint: *input_mint,
        destination: input_vault,
        authority: ctx.payer,
        amount: amount_in,
    };
    let output_leg = SplTransferLeg {
        source: output_vault,
        mint: *output_mint,
        destination: output_account,
        authority: ctx.pool_authority,
        amount: expected_out,
    };
    // Only the hooked mint is pinned to the hook program, and the integrator names exactly the
    // writable extras the hook is allowed to have.
    let policy_for = |leg: &SplTransferLeg| -> (Option<Pubkey>, Vec<Pubkey>) {
        if leg.mint == ctx.hooked_mint {
            (Some(hook.program_id()), hook.allowed_writable(ctx, leg))
        } else {
            (None, Vec::new())
        }
    };
    let (input_program, input_writable) = policy_for(&input_leg);
    let (output_program, output_writable) = policy_for(&output_leg);
    let input = resolve(
        chain,
        LegRole::Input,
        input_leg,
        input_program,
        input_writable,
    )
    .await?;
    let output = resolve(
        chain,
        LegRole::Output,
        output_leg,
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

/// Steps 5 and 6, shared by both AMMs, then the hook's own follow-up steps.
pub(super) async fn swap_checks<C: Chain, B: SwapBuilder>(
    chain: &mut C,
    rec: &mut Recorder,
    builder: &B,
    kit: &SwapKit<'_>,
) -> Result<()> {
    let hook_program = kit.hook.program_id();
    let hooked_account = kit.ctx.trader_accounts[0];
    let quote_account = kit.ctx.trader_accounts[1];

    // Hooked token in, then hooked token out. Each: simulate, check the hook ran once, send.
    for direction in [Direction::HookedIn, Direction::HookedOut] {
        let hooked_in = direction == Direction::HookedIn;
        let label = direction.label();
        let before_hooked = amount_of(chain, &hooked_account).await?;
        let before_quote = amount_of(chain, &quote_account).await?;
        let state_before = match kit.hook.state_account(&kit.ctx.hooked_mint) {
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
                "{label}: swap simulation failed: {:?} {:?}",
                sim.error, sim.logs
            ),
        )?;
        require(
            sim.invocations_of(&hook_program) == 1,
            format!(
                "{label}: the hook must run exactly once (the hooked leg), ran {} times",
                sim.invocations_of(&hook_program)
            ),
        )?;
        let sent = chain
            .send(&transaction, &[])
            .await
            .map_err(|e| DriverError::new(format!("{label}: swap failed: {e}")))?;
        let after_hooked = amount_of(chain, &hooked_account).await?;
        let after_quote = amount_of(chain, &quote_account).await?;
        if hooked_in {
            require(
                after_hooked == before_hooked - SWAP_AMOUNT && after_quote > before_quote,
                format!("{label}: unexpected balances {before_hooked}/{before_quote} -> {after_hooked}/{after_quote}"),
            )?;
        } else {
            require(
                after_quote == before_quote - SWAP_AMOUNT && after_hooked > before_hooked,
                format!("{label}: unexpected balances {before_hooked}/{before_quote} -> {after_hooked}/{after_quote}"),
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
                format!("{label}: the hook's state account {key} did not change"),
            )?;
            detail.push_str("; hook state account changed");
        }
        rec.push(
            &format!("hooked swap ({label})"),
            Some(sent.signature),
            detail,
        );
    }

    // Every swap the hook must refuse.
    for refusal in kit.hook.refusals() {
        let hooked_in = refusal.direction == Direction::HookedIn;
        let label = refusal.direction.label();
        let balances_before = (
            amount_of(chain, &hooked_account).await?,
            amount_of(chain, &quote_account).await?,
        );
        let (amount_in, copies, price) = match refusal.plan {
            RejectionPlan::OverAmount { amount_in } => (amount_in, 1, 0),
            RejectionPlan::RepeatInOneTransaction { times } => (SWAP_AMOUNT, times, 0),
            RejectionPlan::HighPriorityFee {
                amount_in,
                micro_lamports,
            } => (amount_in, 1, micro_lamports),
        };
        let mut instructions = Vec::new();
        for _ in 0..copies {
            instructions.push(
                builder
                    .build(chain, hooked_in, amount_in, amount_in.saturating_sub(2))
                    .await?,
            );
        }
        let transaction = with_budget_and_price(instructions, price);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            !sim.succeeded,
            format!(
                "{label} ({}): the hook was supposed to refuse this swap but it succeeded",
                refusal.plan.label()
            ),
        )?;
        require(
            sim.program_failed_with(&hook_program, refusal.code),
            format!(
                "{label} ({}): the failure did not come from the hook with code {:#x}: {:?}",
                refusal.plan.label(),
                refusal.code,
                sim.logs
            ),
        )?;
        let error = match chain.send(&transaction, &[]).await {
            Ok(_) => {
                return Err(DriverError::new(format!(
                    "{label} ({}): the refused swap was accepted",
                    refusal.plan.label()
                )))
            }
            Err(error) => error,
        };
        require(
            error.custom_code == Some(refusal.code),
            format!(
                "{label} ({}): wrong error from the refused swap: {error}",
                refusal.plan.label()
            ),
        )?;
        let balances_after = (
            amount_of(chain, &hooked_account).await?,
            amount_of(chain, &quote_account).await?,
        );
        require(
            balances_before == balances_after,
            format!("{label}: balances changed after a refused swap: {balances_before:?} -> {balances_after:?}"),
        )?;
        rec.push(
            &format!(
                "hook refused swap ({label}, {}), nothing changed",
                refusal.plan.label()
            ),
            None,
            format!(
                "hook {} rejected with {:#x} after {} hook run(s); {} swap instruction(s) in the transaction rolled back; {}",
                hook_program,
                refusal.code,
                sim.invocations_of(&hook_program),
                copies,
                describe(&sim)
            ),
        );
    }

    run_follow_ups(chain, rec, builder, kit).await
}

/// The hook's own steps after the standard checks: fund a vault, claim, snapshot, expect balances.
async fn run_follow_ups<C: Chain, B: SwapBuilder>(
    chain: &mut C,
    rec: &mut Recorder,
    builder: &B,
    kit: &SwapKit<'_>,
) -> Result<()> {
    for step in kit.hook.follow_up(&kit.ctx) {
        match step {
            FollowUp::Swap {
                label,
                direction,
                amount_in,
            } => {
                let hooked_account = kit.ctx.trader_accounts[0];
                let quote_account = kit.ctx.trader_accounts[1];
                let before = (
                    amount_of(chain, &hooked_account).await?,
                    amount_of(chain, &quote_account).await?,
                );
                let instruction = builder
                    .build(
                        chain,
                        direction == Direction::HookedIn,
                        amount_in,
                        amount_in.saturating_sub(2),
                    )
                    .await?;
                let sent = chain
                    .send(&with_budget(vec![instruction]), &[])
                    .await
                    .map_err(|e| DriverError::new(format!("step `{label}` failed: {e}")))?;
                let after = (
                    amount_of(chain, &hooked_account).await?,
                    amount_of(chain, &quote_account).await?,
                );
                require(
                    after != before,
                    format!("step `{label}`: balances did not change"),
                )?;
                rec.push(
                    &label,
                    Some(sent.signature),
                    format!(
                        "hooked {}->{}, quote {}->{}",
                        before.0, after.0, before.1, after.1
                    ),
                );
            }
            FollowUp::Send {
                label,
                instructions,
                signers,
            } => {
                let refs: Vec<&Keypair> = signers.iter().collect();
                send_step(chain, rec, &label, with_budget(instructions), &refs).await?;
            }
            FollowUp::AdvanceTime(seconds) => {
                chain.advance_time(seconds).await?;
                rec.push(
                    "advance cluster time",
                    None,
                    format!("at least {seconds} seconds"),
                );
            }
            FollowUp::ExpectTokenBalance {
                label,
                account,
                min,
                max,
            } => {
                let amount = amount_of(chain, &account).await?;
                require(
                    (min..=max).contains(&amount),
                    format!("{label}: balance {amount} is outside {min}..={max}"),
                )?;
                rec.push(
                    &label,
                    None,
                    format!("balance {amount} (expected {min}..={max})"),
                );
            }
        }
    }
    Ok(())
}
