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
| `scripts/deploy.sh` | Build and deploy any hook; set up and prove it on the cluster if it ships `examples/devnet.rs`. Must stay generic: no rule-specific options. | Only to fix it. |

## Where business logic belongs

**Start in `rule.rs`.** The starter's rule has two functions:

* `validate_params(params)`: what a creator may configure (called once, at `InitializeHook`; the
  starter has no update instruction).
* `check_transfer(config, context)`: allow (`Ok(())`) or refuse (`Err(HookError)`) one transfer.
  `context` carries amount, source, destination, mint and authority.

Simple rules that fit the existing config shape may only need `rule.rs`, plus rule-specific error
variants (add them, never renumber existing ones), `README.md` and the tests. Rules that introduce
new configuration, state or extra accounts must also update the corresponding plumbing:

| The rule needs | Also change (starter) |
|---|---|
| different settings | `validate_params`, the `HookConfig` params helpers in `config.rs`, `InitializeHookArgs` in `instruction.rs`, `examples/devnet.rs` |
| another extra account (a clock, another PDA) | `CANONICAL_VALIDATION_LIST` and `VALIDATION_LIST_LEN` in `constants.rs`, `config_extra_account_meta` in `pda.rs`, the account count and checks in `processor/execute.rs` |
| state the hook writes | the above, with the account **writable** (a contention decision; see DESIGN.md), and its own layout and init |
| a mutable config | a new update instruction with its own authority and replay checks, and the unauthorized-update tests |

**Do not casually modify** the `Execute` account checks, the `transferring` check, the
validation-list layout and comparison, the config layout, the initialise-authority check,
`hook-kit/`, or anything that widens what accounts a hook accepts or which may be writable. If a rule
seems to need that, stop and explain why to the human before changing it, and add a test for every
check you touch. These are the parts that make a hook safe.

## Starter / hook-kit parity

The plumbing exists twice on purpose: `starter/` carries its own copy so it builds when copied out,
and the templates use `hook-kit/`. A change to shared security behaviour in either one must trigger
a review of the equivalent behaviour in the other, and the PR must say so. This covers: `Execute`
validation and account counts, the Transfer Hook authority check at setup, mint validation
(Token-2022 owner, extension, hook program), token-account validation, the `transferring` check,
validation-list handling, PDA initialisation (including pre-funded addresses), account writability
assumptions, and direct-`Execute` rejection. Do not remove the duplication to "fix" this.

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
8. Can the program be upgraded, the config changed, or the mint's hook selection changed?
9. How can the rule be bypassed?
10. What happens when it rejects?
11. Does it introduce writable-account contention?
12. What are the compute/account-size implications?

Facts to reason with (details in `DESIGN.md`):

* Token-2022 moves the tokens **before** calling the hook. Balances read in a hook are post-transfer.
* A hook only runs for transfers. Burns, mints and owner changes never reach it.
* `Execute` receives the fixed transfer accounts read-only and does not inherit the transfer
  authority's signer privilege, so the hook cannot re-spend the source or destination through that
  authority. Extras are read-only unless declared writable. The hook may still CPI using authorities
  or PDAs it legitimately controls.
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
* unauthorized setup or config change is refused (a non-authority `Initialize`, a second `Initialize`;
  and an unauthorized update if your rule has an update instruction)
* irrelevant transfer path is not affected (a transfer the rule is not about, if the rule has one)
* state update correctness, where the rule keeps state (exact values, not "changed")
* a direct `Execute` call (not from Token-2022) is refused

The starter and every template already contain examples of each. Copy the pattern. Do not delete
the plumbing tests (direct-call rejection, initialise authority, config layout) when you change a rule.

Commands (the same ones CI runs):

```sh
# from the repository root: hook-kit and every template
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
# the standalone starter (or your copy of it)
(cd starter && cargo fmt -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked)
bash -n scripts/deploy.sh
# against the compiled SBF binary, inside one hook's directory
cargo build-sbf --sbf-out-dir target/deploy && SBF_OUT_DIR=$PWD/target/deploy cargo test --locked
```

## Deployment expectations

* Deploy with `scripts/deploy.sh <dir>` (`--help` for options; example options after `--`). Default
  cluster is devnet. The script refuses mainnet without `--allow-mainnet`; that flag does not change
  this rule: **never deploy to mainnet, spend real funds or touch a keypair that is not a throwaway
  devnet key without the human's explicit say-so.**
* Only hooks with `examples/devnet.rs` (today: the starter) are set up and exercised on the cluster
  by the script. For the templates it deploys and stops; say so rather than implying more.
* Never commit keypairs, `.env` files, RPC credentials or `target/`.
* After a deploy, report: cluster, program id, mint, config and validation-list addresses, and who
  holds the upgrade authority and the mint's Transfer Hook authority.
* Report evidence at the level actually reached, and never mark one as another:
  **in-process native** (`cargo test`) < **SBF in-process** (`SBF_OUT_DIR=... cargo test`) <
  **local validator** < **devnet** (deployed, initialised, rejection observed on-chain). A change to
  a rule is not "deployment tested" until it reached local validator or devnet.

## Contribution expectations

A template is a reviewable product, not working Rust. A template pull request contains everything in
`CONTRIBUTING.md`. In particular the template README must have the sections WHAT, WHY, TRIGGER,
EXAMPLE, RULES, LIMITATIONS, TRUST, STATE / COST, TESTS, and DEPLOY / INITIALIZE. Write
LIMITATIONS and TRUST honestly: say how the rule can be bypassed, who can change it, whether the
program can be upgraded, and whether the mint's hook selection can change. Do not describe a rule
as "complete" protection (for example, a per-slot cap is not complete bundle detection).

Keep code in the style of the surrounding code: no new dependencies without need, pinned versions
as in the existing `Cargo.toml`s, `#![forbid(unsafe_code)]`/`deny` where the crate has it, no
panics in on-chain paths (return errors).

## Do not

* Add Raydium code, Raydium program ids, forks, a frontend, a docs site or benchmark infrastructure.
* Claim Raydium supports hooks beyond what Raydium's own docs say.
* Invent error codes that collide with existing ones (starter `0x7001..`, hook-kit `0x8001..`,
  creator-commitment `0xA001..`, fair-launch `0xB001..`, holder-rewards `0xC001..`; a new template
  takes the next free block).
