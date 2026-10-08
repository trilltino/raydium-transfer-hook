# Contributing

This repository **curates** templates. A working piece of Rust is not enough; a template has to be
understandable, tested, and honest about what it cannot do. Small fixes (docs, tests, plumbing bugs)
are welcome as ordinary pull requests with a test.

Before anything else: `cargo test --workspace` (and `cargo test` in `starter/`) must pass, and `cargo fmt --all -- --check` and
`cargo clippy --all-targets -- -D warnings` must be clean.

## Adding a template

A template is a directory under `templates/<name>/` with:

```text
templates/<name>/
  README.md        the nine sections below
  Cargo.toml       like the existing templates (workspace member, hook-kit dependency)
  src/rule.rs      the business logic, pure where you can: no accounts, no Solana types
  src/...          config, instruction, processor, error (codes in the next free block)
  tests/           runtime tests against the real Token-2022 program
```

Your pull request description must tick every box. Copy the checklist from
[`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md), which is filled in
automatically.

* [ ] Clear problem statement: who has this problem, and why is a Transfer Hook the right tool?
* [ ] `rule.rs` implementation
* [ ] Expected behaviour, with a worked example
* [ ] Positive tests (allowed transfers)
* [ ] Rejection tests (exact error code, balances unchanged)
* [ ] Boundary tests (exactly at the limit, one over, first and last instant of any window)
* [ ] Security assumptions
* [ ] Known bypasses and limitations
* [ ] Required extra accounts
* [ ] Writable accounts identified, and the contention they cause
* [ ] Authority model documented (who can configure, who can change)
* [ ] Upgrade assumptions documented (program upgrade authority, mint hook authority)
* [ ] Deployment tested: `scripts/deploy.sh` or your setup on devnet or a local validator, with the
      rejection observed on chain
* [ ] README and example scenario

### The nine README sections

| Section | Answer |
|---|---|
| **WHAT** | What behaviour does this implement? |
| **WHY** | What real problem is it attempting to solve? |
| **TRIGGER** | Which transfers activate the rule? Which do not? |
| **EXAMPLE** | Given state X and transfer Y, what is the result? |
| **RULES** | The exact enforcement behaviour, including the error codes. |
| **LIMITATIONS** | What does this *not* solve? How can it be bypassed? |
| **TRUST** | Who controls the config? Can the program be upgraded? Can the mint's hook selection change? |
| **STATE / COST** | Which accounts are read and written? Contention? What grows compute or account count? |
| **TESTS** | Which tests prove each claim above? |

LIMITATIONS and TRUST are not optional prose. A template that claims more than it enforces is
rejected. For example, per-wallet rules do not establish person-level identity, and a per-slot cap
is not complete bundle detection; say so in the README of any template that relies on them.

### Not accepted

* Templates with no rejection or boundary tests.
* Templates whose README does not say how to bypass them.
* Rules that need unbounded loops, unbounded accounts, or a trusted off-chain party nobody named.
* Raydium program code, program ids or forks. Raydium owns its programs and SDK.
* New dependencies without a reason, or a frontend, docs site or benchmark harness.
* Keypairs, `.env` files, RPC credentials or mainnet transaction artifacts (never commit them).

Ideas that are not ready for a template yet are listed in [`templates/IDEAS.md`](templates/IDEAS.md);
add yours there with the evaluation questions answered.

## Working with an AI agent

Point it at [`AGENTS.md`](AGENTS.md). It lists the questions to answer before generating a hook, the
required tests, and the files it should not touch without asking.
