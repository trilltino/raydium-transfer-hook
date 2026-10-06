# Raydium Transfer Hooks

A local Rust reference harness for exploring Token-2022 Transfer Hook policy, per-transfer account resolution, and Raydium-style transfer paths.

## Scope and status

The workspace now has:

- A platform policy model for disabled, optional, and mandatory hook selection.
- A modular per-mint reference hook engine with configuration validation, transfer rules, and authority-policy checks.
- An SPL-order-aligned resolver boundary that validates typed mint and validation-list state on every transfer and emits ordered account slices.
- CPMM, CLMM, and LaunchLab flow models with local end-to-end tests.

This is **not** a deployed Token-2022 hook program or a patch to Raydium's on-chain programs. The resolver's `TransferHookAccountSource` is an adapter boundary: a real client must use the current SPL interface to fetch and decode accounts and resolve the `ExtraAccountMetaList`. The integration crates model account flow but do not execute Raydium CPIs. See the [upstream ABI review](docs/transfer-surface-matrix.md) before treating any modeled path as production-ready.

## Workspace

- `crates/hook-policy-model` contains platform selection and account metadata types.
- `crates/transfer-hook-sdk` provides fresh per-transfer resolution, validation, and per-leg account ranges.
- `programs/reference-hook` models a configurable per-mint engine; it is not an Anchor entrypoint.
- `integrations/` contains product-path models, not vendored Raydium programs.
- `tests/e2e` exercises policy, resolver, transfer-path planning, lifecycle, and modeled atomic failure.
- `docs/` and `benches/` record source facts, trust assumptions, and measurement guidance.

## Run tests

```powershell
cargo test --workspace
```

The E2E model tests can also be run alone:

```powershell
cargo test -p transfer-hook-e2e
```

## Important ABI boundary

Upstream CPMM transfer helpers currently call `TransferChecked` with only the fixed transfer accounts and do not forward hook extras. Upstream CLMM `SwapV2` consumes remaining accounts as tick arrays and an optional bitmap extension, then stops at the first other account. The public LaunchLab SDK exposes instruction layouts, but its on-chain Rust implementation was not available for this review. Consequently, this workspace keeps deployed V1 layouts untouched and does not claim live hooked Raydium execution. Exact sources, discriminators, account semantics, and decisions are recorded in the [transfer-surface matrix](docs/transfer-surface-matrix.md).
