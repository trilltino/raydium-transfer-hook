# AGENTS.md: development contract for AI coding agents

This file is the instruction set for any AI agent (Claude Code, Codex, Cursor, ...) working in this
repository, whether it is helping a developer build their own hook or contributing a template. Read
it fully before generating code. `CLAUDE.md` only points here.

## What this repository is

A starter kit for **Token-2022 Transfer Hooks**: you design a rule, test it, deploy it to Solana and
optionally contribute it as a template. It is not a Raydium integration and contains no Raydium
program code. The prototype that had some is in git history (tag `pre-community-hook-kit`); do not
resurrect it.

| Path | What | Edit it? |
|---|---|---|
| `starter/` | Standalone copy-me hook. Default rule: max transfer. | **Copy it, then edit `src/rule.rs` in the copy.** Edit the original only to improve the starter itself. |
| `templates/<name>/` | Curated hooks: `README.md`, `src/rule.rs`, `tests/`. | Only when contributing or fixing that template. |
| `hook-kit/` | Shared plumbing used by the templates. | Rarely. A bug here affects every template; add a test with the fix. |
| `scripts/` | `build.sh`, `test.sh`, `deploy.sh`. | Only to fix them. |

## Where business logic belongs

**In `rule.rs`.** The starter's rule has two functions:

* `validate_params(params)`: what a creator may configure (called at init and update).
* `check_transfer(config, context)`: allow (`Ok(())`) or refuse (`Err(HookError)`) one transfer.
  `context` carries amount, source, destination, mint and authority.

Everything else is plumbing: the `Execute` entrypoint, the `transferring`-flag check that rejects
direct calls, PDA and validation-list checks, config encoding and authority modes. In the normal
case you modify `rule.rs`, the error enum (add rule-specific variants, never renumber existing
ones), the config/params helpers if the rule needs new settings, `README.md`, the example in
`examples/devnet.rs` if the setup changes, and the tests.

**Do not casually modify** the `Execute` account checks, the `transferring` check, the
validation-list layout, the config layout, the authority-mode logic, `hook-kit/`, or anything that
widens what accounts a hook accepts. If a rule seems to need that, stop and explain why to the
human before changing it. These are the parts that make a hook safe.

## Before you write a hook: answer these 12 questions

Write the answers down (in the PR description or the template README) before generating code. If
you cannot answer one, ask the human.

1. What behaviour is being enforced?
2. Which transfers should trigger it?
3. Which transfers should not trigger it?
4. What state is required?
5. What extra accounts are required?
6. Which accounts must be writable?
7. Who controls configuration?
8. Can the hook/program be upgraded?
9. How can the rule be bypassed?
10. What happens when it rejects?
11. Does it introduce writable-account contention?
12. What are the compute/account-size implications?

Facts to reason with (details in `DESIGN.md`):

* Token-2022 moves the tokens **before** calling the hook. Balances read in a hook are post-transfer.
* A hook only runs for transfers. Burns, mints and owner changes never reach it.
* The hook's accounts are read-only to it, except extras you declare writable. It cannot spend the
  transfer authority.
* Every writable extra serialises all transfers of that mint on it. Every extra account costs
  transaction bytes and compute; stay well under `hook_kit::PRACTICAL_EXTRA_ACCOUNTS`.
* A rejection aborts the whole transaction, whatever program started the transfer.
* Per-wallet or per-token-account rules are not per-person rules. State that limit in the README.

## Required tests

Every rule has tests for each of these (name them so the intent is visible in `cargo test` output):

* valid transfer is allowed
* rejected transfer fails with the hook's **exact error code**, and balances are unchanged
* **exact boundary** (`amount == limit` and `limit + 1`; the last second of a window; ...)
* malformed config / params are refused (`validate_params`, `Config::decode`)
* unauthorized config update is refused
* irrelevant transfer path is not affected (a transfer the rule is not about, if the rule has one)
* state update correctness, where the rule keeps state (exact values, not "changed")
* a direct `Execute` call (not from Token-2022) is refused

The starter and every template already contain examples of each. Copy the pattern. Do not delete
the plumbing tests (direct-call rejection, authority, config layout) when you change a rule.

Commands:

```sh
cargo test                       # inside the hook's directory; runs against real Token-2022
scripts/test.sh                  # everything in the repository
scripts/test.sh --sbf            # also against the compiled SBF binaries
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
```

## Deployment expectations

* Deploy with `scripts/deploy.sh <dir>`. Default cluster is devnet. **Never deploy to mainnet, spend
  real funds or touch a keypair that is not a throwaway devnet key without the human's explicit say-so.**
* Never commit keypairs, `.env` files, RPC credentials or `target/`.
* After a deploy, report: cluster, program id, mint, config and validation-list addresses, and who
  holds the upgrade authority and the mint's Transfer Hook authority.
* A change to a rule is not "deployment tested" until `deploy.sh` has run it, or the template's own
  setup, on devnet or a local validator and the rejection was observed on-chain.

## Contribution expectations

A template is a reviewable product, not working Rust. A template pull request contains everything in
`CONTRIBUTING.md`. In particular the template README must have the sections WHAT, WHY, TRIGGER,
EXAMPLE, RULES, LIMITATIONS, TRUST, STATE / COST, TESTS. Write LIMITATIONS and TRUST honestly: say
how the rule can be bypassed, who can change it, whether the program can be upgraded, and whether the
mint's hook selection can change. Do not describe a rule as "complete" protection (for example, a
per-slot cap is not complete bundle detection).

Keep code in the style of the surrounding code: no new dependencies without need, pinned versions
as in the existing `Cargo.toml`s, `#![forbid(unsafe_code)]`/`deny` where the crate has it, no
panics in on-chain paths (return errors).

## Do not

* Add Raydium code, Raydium program ids, forks, a frontend, a docs site or benchmark infrastructure.
* Claim Raydium supports hooks beyond what Raydium's own docs say.
* Invent error codes that collide with existing ones (starter `0x7001..`, hook-kit `0x8001..`,
  creator-commitment `0xA001..`, fair-launch `0xB001..`, holder-rewards `0xC001..`; a new template
  takes the next free block).
