<!-- Template PRs: tick every box or say why it does not apply. Other PRs: delete the template checklist. -->

## What does this change?

## Every PR

- [ ] `cargo fmt`, `cargo clippy --locked ... -D warnings` and `cargo test --locked` pass (workspace and `starter/`)
- [ ] Starter / hook-kit parity reviewed if shared hook plumbing changed

## Template checklist (new or changed templates)

- [ ] Clear problem statement (who has it; why a Transfer Hook)
- [ ] `rule.rs` implementation
- [ ] Expected behaviour and a worked example in the README
- [ ] Positive tests
- [ ] Rejection tests (exact error code, balances unchanged)
- [ ] Boundary tests
- [ ] Malformed config / params tests
- [ ] Authority tests (non-authority setup, second setup, unauthorized update if any)
- [ ] Direct `Execute` call refused
- [ ] Known bypasses / limitations
- [ ] Required extra accounts
- [ ] Writable accounts identified (and the contention they cause)
- [ ] Authority model documented (config, program upgrade, mint Transfer Hook authority)
- [ ] README has WHAT / WHY / TRIGGER / EXAMPLE / RULES / LIMITATIONS / TRUST / STATE-COST / TESTS / DEPLOY-INITIALIZE

## The 12 questions (see AGENTS.md)

<!-- Short answers: behaviour, triggers, non-triggers, state, extra accounts, writable accounts,
config control, upgradeability / config / hook-selection changes, bypasses, rejection behaviour,
contention, compute/size. -->

## Evidence

<!-- Highest level reached, never one marked as another: in-process native / SBF in-process /
local validator / devnet. Paste the `cargo test` result; for local validator or devnet, the cluster,
program id, mint and the observed rejection. -->
