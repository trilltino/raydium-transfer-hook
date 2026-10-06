# Source lock and implementation boundary

## Repository baseline

| Source | Revision / version | Use |
|---|---|---|
| `trilltino/raydium-transfer-hook` | `542081e4576c00a3cb74067d1562029b7f8885d0` | Baseline model workspace and existing policy/resolver tests |
| `raydium-io/raydium-cp-swap` | `b3187ae53a1b95a201f855a59024a12ca8f5b51a` | CP-Swap transfer helpers and swap/deposit/withdraw instruction accounts |
| `raydium-io/raydium-clmm` | `ed1eb41519d5355755f7df52b43fa9610938b60b` | SwapV2 remaining-account parser and transfer helpers |
| `raydium-io/raydium-sdk-V2` | `cc33ec28a8921a35609e83293e9e07ad830b0779` | LaunchLab instruction builder/layout facts; not a source for deployed handler behavior |
| `raydium-io/raydium-cpi` | `115df2779d53bacc7db9d0be2773a4b48a6d372b` | Public CPI interface reference |
| `solana-program/token-2022` | `b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e` | Token-2022 transfer-hook extension and CPI behavior |
| `solana-program/transfer-hook` | `ec7063291e968f4b0064e4df0324ff49dcf320df` | Execute ABI, validation-list PDA, TLV resolver and CPI helpers |

The Raydium source links, reviewed instruction facts, and unresolved handler facts are recorded in the [transfer-surface matrix](./transfer-surface-matrix.md). These SHAs are audit references only: this workspace does not vendor or patch the corresponding Raydium programs.

## Executable compatibility line

The on-chain reference program and SDK use the mutually compatible Solana 2.2 / SPL 7 dependency line so they can run under ProgramTest without mixing incompatible account and instruction types.

| Crate | Exact version |
|---|---|
| `solana-program` | `2.2.1` |
| `solana-program-test` | `2.2.7` |
| `solana-sdk` | `2.2.2` |
| `spl-token` | `7.0.0` |
| `spl-token-2022` | `7.0.0` |
| `spl-transfer-hook-interface` | `0.10.0` |
| `spl-tlv-account-resolution` | `0.10.0` |

Cargo.lock records the full transitive dependency resolution. This dependency choice is a reproducible test target, not a claim that these crates are the latest SPL release. The implementation uses the official interface helpers for the Execute account list. ProgramTest 2.2.7 bundles the Token-2022 8.0.0 SBF program; tests use that bundled program to exercise the actual transfer-hook CPI path. The hook under test is registered as a native processor, not loaded from an SBF artifact.

## Covered vertical slice

- `reference-hook-onchain` is a deployable Solana program with an authority-checked per-mint transfer-limit policy.
- The custom policy PDA is derived from `["policy", mint]`; the authority-gated validation-list setup instruction creates the SPL validation PDA and writes that exact read-only policy meta.
- Execute requires the validation list to resolve the policy PDA and rejects direct calls unless Token-2022 has set the source and destination `transferring` flags.
- Execute validates the live mint hook program, Token-2022 source/destination mint and in-transfer flags, validation-list owner/address/TLV account order, and configured limit.
- `transfer-hook-sdk` decodes the current mint extension and delegates TLV/PDA account resolution to `spl-transfer-hook-interface` for each transfer; it does not persist resolution results between calls.
- The ProgramTest case initializes a real Token-2022 mint and token accounts, invokes transfer through the actual Token-2022 processor, and verifies the rejected transfer does not change balances.

The verified ProgramTest run uses ProgramTest's bundled Token-2022 8.0.0 SBF program and loads `reference_hook_onchain.so` from `SBF_OUT_DIR`. It verifies successful hook execution and that rejection leaves source/destination balances unchanged. It is not a `solana-test-validator` run. Without `SBF_OUT_DIR`, ProgramTest falls back to the hook's native processor. The workspace also does not yet contain modified CPMM, CLMM, or LaunchLab program sources, the platform-policy account migration, their hook-aware instruction builders, or the full acceptance matrix from the master prompt.

## Unverified / blocked surfaces

- Raydium CPMM V1 transfer helpers do not currently forward Transfer Hook extras. No live CPMM V2 handler was introduced here.
- CLMM SwapV2 consumes remaining accounts for tick/bitmap data and its transfer helpers do not forward per-leg hook accounts. No live SwapV3 handler was introduced here.
- The reviewed LaunchLab public SDK exposes instruction layouts but not the deployed handler implementation. Migration and fee/vesting transfer behavior remain unverified and unpatched.
- Transfer surfaces beyond the selected swap/deposit/withdraw and SwapV2 paths in the matrix have not all been audited from handler source.
- Arbitrary hook program upgrades, malicious extra-account requirements, v0/v1 message-size limits, compute budgets, and full LaunchLab lifecycle rollback have not been demonstrated by these tests.

Do not advertise any of those surfaces as hook-compatible until the relevant upstream source is available, patched with explicit per-transfer account framing, and covered by runtime tests. No transaction was submitted to a public cluster.
