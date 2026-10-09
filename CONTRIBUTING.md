# Contributing

This repository **curates** templates. A working piece of Rust is not enough; a template has to be
understandable, tested, and honest about what it cannot do.

Contributions accepted into this repository are provided under its [MIT license](LICENSE).
Security issues: report them privately, see [`SECURITY.md`](SECURITY.md).

## Every pull request

Run what CI runs, from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
(cd starter && cargo fmt -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked)
bash -n scripts/deploy.sh
```

CI also builds every hook for SBF and runs its tests against the compiled program.

## A. Docs, tests and plumbing fixes

Ordinary pull requests: describe the change and include a test for any behaviour you fix. If you
change shared hook plumbing (Execute checks, authority checks, mint or token-account validation, the
`transferring` check, validation-list handling, PDA creation, writability assumptions), review the
equivalent code in the other copy (`starter/` vs `hook-kit/`) and say so in the PR; see "Starter /
hook-kit parity" in [`AGENTS.md`](AGENTS.md).

## B. Adding a template

A template is a directory under `templates/<name>/` with:

```text
templates/<name>/
  README.md        the sections below
  Cargo.toml       like the existing templates (workspace member, hook-kit dependency)
  src/rule.rs      the business logic, pure where you can: no accounts, no Solana types
  src/...          config, instruction, processor, error (codes in the next free block)
  tests/           runtime tests against the real Token-2022 program
```

Your pull request description must tick every box. The checklist is in
[`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md), which is filled in
automatically.

* [ ] Clear problem statement: who has this problem, and why is a Transfer Hook the right tool?
* [ ] `rule.rs` implementation
* [ ] Expected behaviour, with a worked example
* [ ] Positive tests (allowed transfers)
* [ ] Rejection tests (exact error code, balances unchanged)
* [ ] Boundary tests (exactly at the limit, one over, first and last instant of any window)
* [ ] Malformed config / params tests
* [ ] Authority tests (non-authority setup, second setup, and unauthorized update if there is one)
* [ ] Direct `Execute` call refused
* [ ] Known bypasses and limitations
* [ ] Required extra accounts
* [ ] Writable accounts identified, and the contention they cause
* [ ] Authority model documented (config authority, program upgrade authority, mint Transfer Hook
      authority, as three separate powers)
* [ ] Evidence level stated honestly (see below)
* [ ] README with every section below

### Evidence levels

State the highest level your template reached, and never mark one as another:

| Level | How |
|---|---|
| in-process (native) | `cargo test` against the real Token-2022 processor |
| SBF in-process | `cargo build-sbf` then `SBF_OUT_DIR=... cargo test` (CI does this for every hook) |
| local validator | deployed and exercised on `solana-test-validator`, rejection observed |
| devnet | deployed, initialised and exercised on devnet, rejection observed on-chain |

Devnet evidence is welcome but not required. A template without a setup example is deployed by
`scripts/deploy.sh` but not initialised or exercised on the cluster; say that rather than claiming
more.

### The README sections

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
| **DEPLOY / INITIALIZE** | How to build, deploy and initialise it; who signs; what to verify afterwards. |

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
