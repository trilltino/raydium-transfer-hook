//! The swap checks both AMMs share: leg resolution, hooked swaps, refused swaps, follow-ups.
//!
//! A flow has one hook for the first mint (`mint_0`), and optionally a second hook for the other
//! mint, so a swap can have one or two hooked legs. "Direction" in the builders always means
//! whether `mint_0` is the swap's input.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair};
use transfer_hook_sdk::{LegHook, LegRole, SplTransferLeg};

use super::{recorder::Recorder, support::*, SWAP_AMOUNT};
use crate::{
    chain::{Chain, DriverError, Result, Simulation},
    hooks::{Direction, FollowUp, HookContext, HookSetup, RejectionPlan},
};

/// One hook and the context of the mint it is on.
pub(super) struct HookEntry<'a> {
    pub(super) hook: &'a dyn HookSetup,
    pub(super) ctx: HookContext,
}

/// What a flow needs to know to run the swap checks, independent of the AMM. The first entry is
/// the hook on `mint_0`; a second entry is a hook on the other mint.
pub(super) struct SwapKit<'a> {
    pub(super) hooks: Vec<HookEntry<'a>>,
}

impl SwapKit<'_> {
    /// The hook on `mint_0`, whose context also carries the trader accounts and the vaults.
    pub(super) fn primary(&self) -> &HookEntry<'_> {
        &self.hooks[0]
    }

    /// The hook on `mint`, if that mint has one.
    pub(super) fn entry_for(&self, mint: &Pubkey) -> Option<&HookEntry<'_>> {
        self.hooks
            .iter()
            .find(|entry| &entry.ctx.hooked_mint == mint)
    }

    fn is_dual(&self) -> bool {
        self.hooks.len() > 1
    }
}

/// Build the framed swap instruction(s) for one direction. `mint0_in` is true when `mint_0` (the
/// first hooked token) is the input.
pub(super) trait SwapBuilder {
    async fn build<C: Chain>(
        &self,
        chain: &C,
        mint0_in: bool,
        amount_in: u64,
        expected_out: u64,
    ) -> Result<Instruction>;

    /// An exact-output swap: receive exactly `amount_out`, spend at most `max_amount_in`. Only
    /// AMMs with such an instruction implement it.
    async fn build_exact_output<C: Chain>(
        &self,
        _chain: &C,
        _mint0_in: bool,
        _max_amount_in: u64,
        _amount_out: u64,
    ) -> Result<Instruction> {
        Err(DriverError::new(
            "this AMM has no exact-output swap that supports hooked mints",
        ))
    }
}

/// Resolve the two transfer legs of a swap independently. Each leg resolves against the hook of
/// its own mint (if it has one), and a leg whose mint has no hook must come back unhooked.
#[allow(clippy::too_many_arguments)]
pub(super) async fn legs<C: Chain>(
    chain: &C,
    kit: &SwapKit<'_>,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    input_account: Pubkey,
    output_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    amount_in: u64,
    expected_out: u64,
) -> Result<(LegHook, LegHook)> {
    let primary = &kit.primary().ctx;
    let input_leg = SplTransferLeg {
        source: input_account,
        mint: *input_mint,
        destination: input_vault,
        authority: primary.payer,
        amount: amount_in,
    };
    let output_leg = SplTransferLeg {
        source: output_vault,
        mint: *output_mint,
        destination: output_account,
        authority: primary.pool_authority,
        amount: expected_out,
    };
    // A hooked mint is pinned to its hook program, and the integrator names exactly the writable
    // extras that hook is allowed to have.
    let policy_for = |leg: &SplTransferLeg| -> (Option<Pubkey>, Vec<Pubkey>) {
        match kit.entry_for(&leg.mint) {
            Some(entry) => (
                Some(entry.hook.program_id()),
                entry.hook.allowed_writable(&entry.ctx, leg),
            ),
            None => (None, Vec::new()),
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
    let (input_should_hook, output_should_hook) = (
        kit.entry_for(input_mint).is_some(),
        kit.entry_for(output_mint).is_some(),
    );
    require(
        input.is_hooked() == input_should_hook && output.is_hooked() == output_should_hook,
        format!(
            "exactly the hooked legs must resolve a hook (input hooked: {} expected {}, output hooked: {} expected {})",
            input.is_hooked(),
            input_should_hook,
            output.is_hooked(),
            output_should_hook
        ),
    )?;
    Ok((input, output))
}

/// Resolve the two transfer legs of a two-token operation (liquidity, fee collection, pool
/// creation) independently, like [`legs`] does for a swap: each leg resolves against the hook of
/// its own mint, and a mint without a hook must come back unhooked.
pub(super) async fn pair_legs<C: Chain>(
    chain: &C,
    kit: &SwapKit<'_>,
    token_0: SplTransferLeg,
    token_1: SplTransferLeg,
) -> Result<(LegHook, LegHook)> {
    let mut resolved = Vec::new();
    for (role, leg) in [(LegRole::Token0, token_0), (LegRole::Token1, token_1)] {
        let (program, writable) = match kit.entry_for(&leg.mint) {
            Some(entry) => (
                Some(entry.hook.program_id()),
                entry.hook.allowed_writable(&entry.ctx, &leg),
            ),
            None => (None, Vec::new()),
        };
        let hooked = leg.mint;
        let leg_hook = resolve(chain, role, leg, program, writable).await?;
        require(
            leg_hook.is_hooked() == kit.entry_for(&hooked).is_some(),
            format!(
                "exactly the hooked mint must resolve a hook ({role}: hooked {}, expected {})",
                leg_hook.is_hooked(),
                kit.entry_for(&hooked).is_some()
            ),
        )?;
        resolved.push(leg_hook);
    }
    let second = resolved.pop().expect("two legs");
    let first = resolved.pop().expect("two legs");
    Ok((first, second))
}

/// Approve the hooked mints the way an operator does (`raydium-hook mint approve`, the same code),
/// after proving that the program refuses to create a pool with them first. `unapproved_pool` is the
/// pool-creation instruction built without the approval records.
pub(super) async fn approve_hooked_mints<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    env: &crate::env::Environment,
    amm: crate::approval::Amm,
    step: &str,
    mints: &[Pubkey],
    unapproved_pool: Instruction,
) -> Result<()> {
    use crate::approval::{self, Outcome, RecordState};

    let code = amm.not_support_mint_code();
    let program = unapproved_pool.program_id;
    let gate = chain
        .simulate(&with_budget(vec![unapproved_pool]), &[])
        .await?;
    require(
        !gate.succeeded && gate.program_failed_with(&program, code),
        format!(
            "creating a pool with an unapproved hooked mint must be refused with error {code}: {:?} {:?}",
            gate.error, gate.logs
        ),
    )?;
    rec.push(
        "pool creation refused until the hooked mint is approved",
        None,
        format!("rejected with error {code}; {}", describe(&gate)),
    );

    for row in approval::approve(chain, env, &[amm], mints, false).await? {
        match row.outcome {
            Outcome::Approved { signature } => rec.push(step, Some(signature), ""),
            Outcome::AlreadyApproved => rec.push(step, None, "already approved"),
            other => {
                return Err(DriverError::new(format!(
                    "approving {} on {} did not succeed: {other}",
                    row.mint,
                    amm.name()
                )))
            }
        }
    }
    for mint in mints {
        require(
            approval::state(chain, env, amm, mint).await? == RecordState::Approved,
            format!(
                "{mint} is not approved on {} after approving it",
                amm.name()
            ),
        )?;
    }
    Ok(())
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

/// How many times each hook program must run in one swap: once per hooked leg that uses it.
pub(super) fn expected_runs(kit: &SwapKit<'_>) -> Vec<(Pubkey, usize)> {
    let mut runs: Vec<(Pubkey, usize)> = Vec::new();
    for entry in &kit.hooks {
        let program = entry.hook.program_id();
        match runs.iter_mut().find(|(p, _)| *p == program) {
            Some((_, count)) => *count += 1,
            None => runs.push((program, 1)),
        }
    }
    runs
}

/// Steps 5 and 6, shared by both AMMs, then the hook's own follow-up steps.
pub(super) async fn swap_checks<C: Chain, B: SwapBuilder>(
    chain: &mut C,
    rec: &mut Recorder,
    builder: &B,
    kit: &SwapKit<'_>,
) -> Result<()> {
    let hooked_account = kit.primary().ctx.trader_accounts[0];
    let quote_account = kit.primary().ctx.trader_accounts[1];
    let runs = expected_runs(kit);

    // mint_0 in, then mint_0 out. Each: simulate, check every hook ran as often as it has hooked
    // legs, send, check the balances and any hook state.
    for direction in [Direction::HookedIn, Direction::HookedOut] {
        let mint0_in = direction == Direction::HookedIn;
        let label = direction.label();
        let before_hooked = amount_of(chain, &hooked_account).await?;
        let before_quote = amount_of(chain, &quote_account).await?;
        let mut states_before = Vec::new();
        for entry in &kit.hooks {
            if let Some(key) = entry.hook.state_account(&entry.ctx.hooked_mint) {
                states_before.push((key, raw_data(chain, &key).await?));
            }
        }
        let instruction = builder
            .build(chain, mint0_in, SWAP_AMOUNT, SWAP_AMOUNT.saturating_sub(2))
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
        for (program, expected) in &runs {
            require(
                sim.invocations_of(program) == *expected,
                format!(
                    "{label}: hook {program} must run {expected} time(s) (once per hooked leg), ran {} times",
                    sim.invocations_of(program)
                ),
            )?;
        }
        let sent = chain
            .send(&transaction, &[])
            .await
            .map_err(|e| DriverError::new(format!("{label}: swap failed: {e}")))?;
        let after_hooked = amount_of(chain, &hooked_account).await?;
        let after_quote = amount_of(chain, &quote_account).await?;
        if mint0_in {
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
        let runs_text = if kit.is_dual() {
            "both legs hooked, each hook ran per its leg"
        } else {
            "hook ran once"
        };
        let mut detail = format!(
            "{runs_text}; hooked {before_hooked}->{after_hooked}, quote {before_quote}->{after_quote}; {}",
            describe(&sim)
        );
        let mut changed = 0;
        for (key, before) in &states_before {
            let after = raw_data(chain, key).await?;
            require(
                &after != before,
                format!("{label}: the hook's state account {key} did not change"),
            )?;
            changed += 1;
        }
        if changed > 0 {
            detail.push_str("; hook state account changed");
        }
        rec.push(
            &format!("hooked swap ({label})"),
            Some(sent.signature),
            detail,
        );
    }

    // Every swap a hook must refuse. With two hooked legs only the refusals where the refusing
    // hook's own mint is the *input* are run: that transfer is first, so the failure can only come
    // from that hook. Refusals on the output leg are covered by the single-hook flows.
    for (index, entry) in kit.hooks.iter().enumerate() {
        let hook_program = entry.hook.program_id();
        for refusal in entry.hook.refusals() {
            let own_mint_in = refusal.direction == Direction::HookedIn;
            if kit.is_dual() && !own_mint_in {
                continue;
            }
            // `mint0_in` for the builder: the hook's own mint is the input exactly when its
            // direction says so, and mint_0 is the first hook's mint.
            let mint0_in = own_mint_in == (index == 0);
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
                        .build(chain, mint0_in, amount_in, amount_in.saturating_sub(2))
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
    }

    if kit.is_dual() {
        require(
            kit.hooks
                .iter()
                .all(|entry| entry.hook.follow_up(&entry.ctx).is_empty()),
            "follow-up steps are not supported when both mints are hooked",
        )?;
        return Ok(());
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
    let primary = kit.primary();
    let mut remembered: std::collections::HashMap<String, u64> = Default::default();
    for step in primary.hook.follow_up(&primary.ctx) {
        match step {
            FollowUp::Remember { name, account } => {
                let amount = amount_of(chain, &account).await?;
                remembered.insert(name, amount);
            }
            FollowUp::ExpectChange {
                label,
                account,
                since,
                min,
                max,
            } => {
                let before = *remembered
                    .get(&since)
                    .ok_or_else(|| DriverError::new(format!("nothing remembered as `{since}`")))?;
                let now = amount_of(chain, &account).await?;
                let change = i128::from(now) - i128::from(before);
                require(
                    (i128::from(min)..=i128::from(max)).contains(&change),
                    format!("{label}: balance changed by {change}, outside {min}..={max}"),
                )?;
                rec.push(
                    &label,
                    None,
                    format!("balance {before} -> {now} (change {change}, expected {min}..={max})"),
                );
            }
            FollowUp::SendExpectFailure {
                label,
                instructions,
                signers,
                code,
            } => {
                let refs: Vec<&Keypair> = signers.iter().collect();
                let hook_program = primary.hook.program_id();
                let transaction = with_budget(instructions);
                let sim = chain.simulate(&transaction, &refs).await?;
                require(
                    !sim.succeeded,
                    format!(
                        "step `{label}`: the hook was supposed to refuse this but it succeeded"
                    ),
                )?;
                require(
                    sim.program_failed_with(&hook_program, code),
                    format!(
                        "step `{label}`: the failure did not come from the hook with code {code:#x}: {:?}",
                        sim.logs
                    ),
                )?;
                let error = match chain.send(&transaction, &refs).await {
                    Ok(_) => {
                        return Err(DriverError::new(format!(
                            "step `{label}`: the refused transaction was accepted"
                        )))
                    }
                    Err(error) => error,
                };
                require(
                    error.custom_code == Some(code),
                    format!("step `{label}`: wrong error: {error}"),
                )?;
                rec.push(
                    &label,
                    None,
                    format!("refused by the hook with {code:#x}; {}", describe(&sim)),
                );
            }
            FollowUp::Swap {
                label,
                direction,
                amount_in,
            } => {
                let hooked_account = primary.ctx.trader_accounts[0];
                let quote_account = primary.ctx.trader_accounts[1];
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

/// The hooks of a flow with the context of each hooked mint: `mint_0` first, then (if the flow has
/// a second hook) the other mint, whose context sees the trader accounts and the vaults swapped so
/// that "its" token comes first.
pub(super) fn hook_entries<'a>(
    inputs: &super::FlowInputs<'a>,
    world: &super::world::World,
    payer: Pubkey,
    pool_authority: Pubkey,
    vaults: [Pubkey; 2],
    now: i64,
) -> Vec<HookEntry<'a>> {
    use solana_sdk::signature::Signer;
    let traders = [world.trader[0].pubkey(), world.trader[1].pubkey()];
    let mut entries = vec![HookEntry {
        hook: inputs.hook,
        ctx: HookContext {
            payer,
            hooked_mint: world.hooked.pubkey(),
            quote_mint: world.quote.pubkey(),
            trader_accounts: traders,
            pool_authority,
            vaults,
            now,
        },
    }];
    if let Some(second) = inputs.second_hook {
        entries.push(HookEntry {
            hook: second,
            ctx: HookContext {
                payer,
                hooked_mint: world.quote.pubkey(),
                quote_mint: world.hooked.pubkey(),
                trader_accounts: [traders[1], traders[0]],
                pool_authority,
                vaults: [vaults[1], vaults[0]],
                now,
            },
        });
    }
    entries
}

/// What to leave behind after a flow, so the pool can be traded against afterwards.
pub(super) fn build_session(
    amm: &str,
    world: &super::world::World,
    kit: &SwapKit<'_>,
) -> crate::session::Session {
    use solana_sdk::signature::Signer;
    let mut allowed: Vec<Pubkey> = Vec::new();
    for entry in &kit.hooks {
        let ctx = &entry.ctx;
        for (source, destination) in [
            (ctx.trader_accounts[0], ctx.vaults[0]),
            (ctx.vaults[0], ctx.trader_accounts[0]),
        ] {
            let leg = SplTransferLeg {
                source,
                mint: ctx.hooked_mint,
                destination,
                authority: ctx.payer,
                amount: 0,
            };
            for key in entry.hook.allowed_writable(ctx, &leg) {
                if !allowed.contains(&key) {
                    allowed.push(key);
                }
            }
        }
    }
    let primary = kit.primary();
    crate::session::Session {
        amm: amm.to_string(),
        mint_0: world.hooked.pubkey().to_string(),
        mint_1: world.quote.pubkey().to_string(),
        accounts: [
            primary.ctx.trader_accounts[0].to_string(),
            primary.ctx.trader_accounts[1].to_string(),
        ],
        hooks: kit
            .hooks
            .iter()
            .map(|entry| crate::session::SessionHook {
                mint: entry.ctx.hooked_mint.to_string(),
                program: entry.hook.program_id().to_string(),
            })
            .collect(),
        allowed_writable: allowed.iter().map(|k| k.to_string()).collect(),
        pool: None,
    }
}

/// An exact-output swap in each direction: the hook must run once per hooked leg, the trader must
/// receive exactly the requested amount (when no transfer fee is withheld) and spend no more than
/// the limit, and a limit too low to cover the price must be refused with nothing moved.
pub(super) async fn exact_output_checks<C: Chain, B: SwapBuilder>(
    chain: &mut C,
    rec: &mut Recorder,
    builder: &B,
    kit: &SwapKit<'_>,
    exact: bool,
) -> Result<()> {
    const AMOUNT_OUT: u64 = 5;
    // Well above the price of AMOUNT_OUT on the seeded pool (about 6 with fees), and within every
    // hook's per-swap limit.
    const MAX_IN: u64 = SWAP_AMOUNT;
    let hooked_account = kit.primary().ctx.trader_accounts[0];
    let quote_account = kit.primary().ctx.trader_accounts[1];
    let runs = expected_runs(kit);
    for direction in [Direction::HookedIn, Direction::HookedOut] {
        let mint0_in = direction == Direction::HookedIn;
        let label = format!("exact output, {}", direction.label());
        let (in_account, out_account) = if mint0_in {
            (hooked_account, quote_account)
        } else {
            (quote_account, hooked_account)
        };
        let before_in = amount_of(chain, &in_account).await?;
        let before_out = amount_of(chain, &out_account).await?;
        let instruction = builder
            .build_exact_output(chain, mint0_in, MAX_IN, AMOUNT_OUT)
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
        for (program, expected) in &runs {
            require(
                sim.invocations_of(program) == *expected,
                format!(
                    "{label}: hook {program} must run {expected} time(s), ran {} times",
                    sim.invocations_of(program)
                ),
            )?;
        }
        let sent = chain
            .send(&transaction, &[])
            .await
            .map_err(|e| DriverError::new(format!("{label}: swap failed: {e}")))?;
        let after_in = amount_of(chain, &in_account).await?;
        let after_out = amount_of(chain, &out_account).await?;
        let spent = before_in.saturating_sub(after_in);
        let received = after_out.saturating_sub(before_out);
        require(
            spent > 0 && spent <= MAX_IN,
            format!("{label}: spent {spent}, expected 1..={MAX_IN}"),
        )?;
        require(
            if exact {
                received == AMOUNT_OUT
            } else {
                received > 0 && received <= AMOUNT_OUT
            },
            format!("{label}: received {received}, expected {AMOUNT_OUT}"),
        )?;
        rec.push(
            &format!("hooked exact-output swap ({label})"),
            Some(sent.signature),
            format!(
                "received {received} for {spent} (limit {MAX_IN}); every hook ran once per hooked leg; {}",
                describe(&sim)
            ),
        );

        // A limit of 1 cannot buy AMOUNT_OUT: the swap is refused and nothing moves.
        let instruction = builder
            .build_exact_output(chain, mint0_in, 1, AMOUNT_OUT)
            .await?;
        let transaction = with_budget(vec![instruction]);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            !sim.succeeded,
            format!("{label}: a limit of 1 must not buy {AMOUNT_OUT}"),
        )?;
        let balances = (
            amount_of(chain, &in_account).await?,
            amount_of(chain, &out_account).await?,
        );
        require(
            balances == (after_in, after_out),
            format!("{label}: balances moved after a refused exact-output swap"),
        )?;
        rec.push(
            &format!("exact-output swap over its limit refused ({label})"),
            None,
            describe(&sim),
        );
    }
    Ok(())
}
