# Changelog

All notable changes. This project is pre-1.0: interfaces and account layouts may change between
minor versions. Maturity labels are defined in [`templates/README.md`](templates/README.md).

## v0.1.0 (2026-10-09)

First tagged release of the community Transfer Hook kit (independent, not affiliated with Raydium,
not audited).

**Kit**

* `starter/` (stable): a standalone, copyable Token-2022 Transfer Hook (default rule: max transfer)
  with `examples/devnet.rs`.
* `hook-kit/`: shared plumbing for the templates: `execute_prelude` (the `transferring` check,
  read-only fixed accounts, exact account count, byte-for-byte validation list), compile-time
  canonical validation lists, mint and token-account reads, PDA creation, test helpers.
* `scripts/deploy.sh`: build and deploy any hook; prove it on the cluster when it ships
  `examples/devnet.rs`; generic flags with example options after `--`; mainnet refused without
  `--allow-mainnet`.
* AGENTS.md contract for AI agents, CONTRIBUTING with evidence levels, SECURITY.md, issue templates.

**Templates**

* `creator-commitment` (stable): a committed token account cannot fall below its vesting floor.
* `fair-launch` (reference): launch participation controls (buy size, balance, buys per slot,
  priority fee) inside a window. Open question: venue ordering with a real AMM.
* `holder-rewards` (experimental): balance x time rewards with a global index, one-time or
  ongoing funding, extension vetting, and `Reconcile` for burned or closed accounts.

**Evidence at this tag**

* CI: fmt, clippy, tests in-process, tests against the SBF builds of every hook, MSRV (Rust 1.88).
* Devnet (2026-10-09, throwaway keys): the starter and Creator Commitment deployed, initialised
  and exercised through `scripts/deploy.sh`, with the refusal observed on-chain. Fair Launch and
  Holder Rewards: in-process and SBF only.
