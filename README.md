# Raydium Transfer Hooks

A Rust reference harness for Token-2022 Transfer Hook policy, real Token-2022 execution, account resolution, and Raydium-style transfer paths.

## Scope and status

The workspace now has:

- A platform policy model for disabled, optional, and mandatory hook selection.
- A modular per-mint reference hook engine with configuration validation, transfer rules, and authority-policy checks.
- A deployable reference hook program with an authority-checked per-mint transfer limit and SPL `Execute` account validation.
- An SDK resolver that reads the current Token-2022 mint extension and uses the official SPL TLV/account-resolution helper for each transfer.
- Hook-aware instruction framing/builders for CPMM `swap_base_input_v2` and CLMM `swap_v3`.
- Pinned Apache-2.0 CP-Swap and CLMM handler snapshots with versioned swap entrypoints, CPI account forwarding, and fail-closed behavior on unsupported helper paths.
- CPMM, CLMM, and LaunchLab flow models with local end-to-end tests.
- A ProgramTest case using SBF Token-2022 and the built SBF hook to prove successful hook execution and atomic rollback after hook rejection.

The runtime test uses ProgramTest's bundled Token-2022 8.0.0 SBF binary and loads the hook's SBF build. It proves real Token-2022 hook invocation and rollback. A separate smoke test confirms ProgramTest loads the vendored CPMM and CLMM SBF artifacts, but their swap instructions have not yet been executed with a hooked mint. The integration crates still model product account flow and do not execute Raydium CPIs. LaunchLab remains model-only because its on-chain handler is closed source. See the [source lock and implementation boundary](docs/source-lock.md) and [upstream ABI review](docs/transfer-surface-matrix.md) before treating any modeled path as production-ready.

## Workspace

- `crates/hook-policy-model` contains platform selection and account metadata types.
- `crates/transfer-hook-sdk` provides fresh SPL account resolution and constructs framed CPMM/CLMM swap instructions.
- `programs/reference-hook` is the policy model; `programs/reference-hook-onchain` is the deployable Solana program.
- `vendor/raydium-cp-swap` and `vendor/raydium-clmm` contain pinned upstream source snapshots with hook-aware versioned swap paths; test each snapshot with its own Cargo manifest.
- `integrations/` contains product-path models, not live Raydium program tests.
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

For Raydium artifact loading and the reference hook transaction, point `SBF_OUT_DIR` at a directory containing all three SBF artifacts:

```powershell
cargo build-sbf --manifest-path programs\reference-hook-onchain\Cargo.toml --sbf-out-dir target\raydium-runtime-sbf
cargo build-sbf --manifest-path vendor\raydium-cp-swap\programs\cp-swap\Cargo.toml --sbf-out-dir target\raydium-runtime-sbf
cargo build-sbf --manifest-path vendor\raydium-clmm\programs\amm\Cargo.toml --sbf-out-dir target\raydium-runtime-sbf
$env:SBF_OUT_DIR = (Resolve-Path target\raydium-runtime-sbf).Path
cargo test --locked --target-dir target\hook-target -p reference-hook-onchain --test raydium_sbf_smoke -- --ignored --nocapture
cargo test --locked --target-dir target\hook-target -p reference-hook-onchain --test token_2022_transfer -- --nocapture
Remove-Item Env:SBF_OUT_DIR
```

The ignored smoke test requires the artifacts and only verifies that the Raydium SBF programs register as executable accounts; it is not evidence of swap execution or CPI hook forwarding. The pinned handler snapshots have separate unit-test targets:

```powershell
cargo test --locked --manifest-path vendor\raydium-cp-swap\Cargo.toml -p raydium-cp-swap --lib
cargo test --locked --manifest-path vendor\raydium-clmm\Cargo.toml -p raydium-clmm --lib
cargo build-sbf --manifest-path vendor\raydium-clmm\programs\amm\Cargo.toml --sbf-out-dir target\clmm-hook-sbf
```

The E2E model tests can also be run alone:

```powershell
cargo test -p transfer-hook-e2e
```

The focused ProgramTest uses the SBF artifact when `SBF_OUT_DIR` points to `target\deploy`; without it, ProgramTest falls back to the native hook processor.

## Important ABI and runtime boundary

Existing CPMM V1 and CLMM SwapV2 layouts remain unchanged. Hooked swaps use CPMM `swap_base_input_v2` and CLMM `swap_v3`, which frame tick/bitmap and per-transfer hook account tails explicitly. Other helper-based CP-Swap/CLMM transfer paths fail closed on hook-enabled mints, and CLMM limit-order direct CPIs explicitly reject them. The public LaunchLab SDK exposes instruction layouts, but its on-chain Rust implementation was not available for this review. ProgramTest now confirms it can load both Raydium SBF artifacts, but the versioned swap instructions have not yet been executed there; do not infer live end-to-end success from this smoke test, unit tests, or SBF compilation. Exact sources, discriminators, account semantics, and decisions are recorded in the [transfer-surface matrix](docs/transfer-surface-matrix.md).
