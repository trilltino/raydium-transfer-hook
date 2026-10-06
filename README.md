# Raydium Transfer Hooks

A Rust reference harness for Token-2022 Transfer Hook policy, real Token-2022 execution, account resolution, and Raydium-style transfer paths.

## Scope and status

The workspace now has:

- A platform policy model for disabled, optional, and mandatory hook selection.
- A modular per-mint reference hook engine with configuration validation, transfer rules, and authority-policy checks.
- A deployable reference hook program with an authority-checked per-mint transfer limit and SPL `Execute` account validation.
- An SDK resolver that reads the current Token-2022 mint extension and uses the official SPL TLV/account-resolution helper for each transfer.
- CPMM, CLMM, and LaunchLab flow models with local end-to-end tests.
- A ProgramTest case using SBF Token-2022 and the built SBF hook to prove successful hook execution and atomic rollback after hook rejection.

The runtime test uses ProgramTest's bundled Token-2022 8.0.0 SBF binary and loads the hook's SBF build. It proves real Token-2022 hook invocation and rollback. This is **not** a patch to Raydium's on-chain programs. The integration crates still model product account flow and do not execute Raydium CPIs. See the [source lock and implementation boundary](docs/source-lock.md) and [upstream ABI review](docs/transfer-surface-matrix.md) before treating any modeled path as production-ready.

## Workspace

- `crates/hook-policy-model` contains platform selection and account metadata types.
- `crates/transfer-hook-sdk` provides fresh per-transfer resolution, validation, and per-leg account ranges.
- `programs/reference-hook` is the policy model; `programs/reference-hook-onchain` is the deployable Solana program.
- `integrations/` contains product-path models, not vendored Raydium programs.
- `tests/e2e` exercises policy, resolver, transfer-path planning, lifecycle, and modeled atomic failure.
- `docs/` and `benches/` record source facts, trust assumptions, and measurement guidance.

## Run tests

```powershell
cargo test --workspace
cargo build-sbf --manifest-path programs\reference-hook-onchain\Cargo.toml
$env:SBF_OUT_DIR = (Resolve-Path target\deploy).Path
cargo test -p reference-hook-onchain --test token_2022_transfer
Remove-Item Env:SBF_OUT_DIR
```

The E2E model tests can also be run alone:

```powershell
cargo test -p transfer-hook-e2e
```

The focused ProgramTest uses the SBF artifact when `SBF_OUT_DIR` points to `target\deploy`; without it, ProgramTest falls back to the native hook processor.

## Important ABI boundary

Upstream CPMM transfer helpers currently call `TransferChecked` with only the fixed transfer accounts and do not forward hook extras. Upstream CLMM `SwapV2` consumes remaining accounts as tick arrays and an optional bitmap extension, then stops at the first other account. The public LaunchLab SDK exposes instruction layouts, but its on-chain Rust implementation was not available for this review. Consequently, this workspace keeps deployed V1 layouts untouched and does not claim live hooked Raydium execution. Exact sources, discriminators, account semantics, and decisions are recorded in the [transfer-surface matrix](docs/transfer-surface-matrix.md).
