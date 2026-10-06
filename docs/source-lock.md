# Source lock and implementation boundary

## Repository baseline

| Source | Revision / version | Use |
|---|---|---|
| `trilltino/raydium-transfer-hook` | `542081e4576c00a3cb74067d1562029b7f8885d0` | Baseline model workspace and existing policy/resolver tests |
| `raydium-io/raydium-cp-swap` | `b3187ae53a1b95a201f855a59024a12ca8f5b51a` | Apache-2.0 source vendored at `vendor/raydium-cp-swap`; CP-Swap handlers and transfer helpers |
| `raydium-io/raydium-clmm` | `ed1eb41519d5355755f7df52b43fa9610938b60b` | Apache-2.0 source vendored at `vendor/raydium-clmm`; CLMM handlers and transfer helpers |
| `raydium-io/raydium-sdk-V2` | `cc33ec28a8921a35609e83293e9e07ad830b0779` | GPL-3.0 SDK reference only; not vendored and not evidence of deployed handler behavior |
| `raydium-io/raydium-cpi` | `115df2779d53bacc7db9d0be2773a4b48a6d372b` | Public CPI interface reference |
| `solana-program/token-2022` | `b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e` | Token-2022 transfer-hook extension and CPI behavior |
| `solana-program/transfer-hook` | `ec7063291e968f4b0064e4df0324ff49dcf320df` | Execute ABI, validation-list PDA, TLV resolver and CPI helpers |

The exact CPMM and CLMM source snapshots are vendored with their upstream Apache-2.0 license notices. Their versioned swap handlers and transfer helpers are patched in place and are not included in the root Cargo workspace; validate them with their own manifest. The SDK revision is an audit reference only; its GPL-3.0 license requires a separate compatibility review before any source is incorporated. The Raydium source links, reviewed instruction facts, and unresolved LaunchLab handler facts are recorded in the [transfer-surface matrix](./transfer-surface-matrix.md).

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

Cargo.lock records the full transitive dependency resolution. This dependency choice is a reproducible test target, not a claim that these crates are the latest SPL release. The implementation uses the official interface helpers for the Execute account list. ProgramTest 2.2.7 bundles the Token-2022 8.0.0 SBF program; tests use that bundled program to exercise the actual transfer-hook CPI path. The reference hook can run as a native processor by default or load its SBF artifact when `SBF_OUT_DIR` is configured; the verified run below used the SBF artifact.

## Covered vertical slice

- `reference-hook-onchain` is a deployable Solana program with an authority-checked per-mint transfer-limit policy.
- The custom policy PDA is derived from `["policy", mint]`; the authority-gated validation-list setup instruction creates the SPL validation PDA and writes that exact read-only policy meta.
- Execute requires the validation list to resolve the policy PDA and rejects direct calls unless Token-2022 has set the source and destination `transferring` flags.
- Execute validates the live mint hook program, Token-2022 source/destination mint and in-transfer flags, validation-list owner/address/TLV account order, and configured limit.
- `transfer-hook-sdk` decodes the current mint extension and delegates TLV/PDA account resolution to `spl-transfer-hook-interface` for each transfer; it does not persist resolution results between calls. It also reframes caller-built CPMM V1 and CLMM SwapV2 instructions as CPMM V2 and CLMM SwapV3 with explicit per-leg slice counts.
- The ProgramTest case initializes a real Token-2022 mint and token accounts, invokes transfer through the actual Token-2022 processor, and verifies the rejected transfer does not change balances.

The verified ProgramTest run uses ProgramTest's bundled Token-2022 8.0.0 SBF program and loads `reference_hook_onchain.so` from `SBF_OUT_DIR`. The focused test log confirms this path and hook Execute invocation; it verifies successful hook execution and that rejection leaves source/destination balances unchanged. It is not a `solana-test-validator` run. Without `SBF_OUT_DIR`, ProgramTest falls back to the hook's native processor. CPMM V2 and CLMM SwapV3 source-level handlers/builders now exist and pass their local tests, but no ProgramTest has yet executed either Raydium program or proven their CPI forwarding with Token-2022. The workspace has no LaunchLab handler source or full acceptance matrix from the master prompt.

On the current Windows SBF toolchain, `cargo build-sbf` exits successfully but emits maximum-frame-size diagnostics for dependency-generated symbols. The runtime test loads and executes the resulting hook artifact, but those diagnostics remain a release-readiness item and must be understood against the pinned dependency/toolchain set before deployment.

## Unverified / blocked surfaces

- CPMM `swap_base_input_v2` and CLMM `swap_v3` are implemented in the pinned vendor snapshots with explicit section counts and per-transfer CPI forwarding. Their V1/SwapV2 counterparts remain unchanged and helper-based non-versioned transfers reject hooked mints. CLMM direct limit-order open/increase/settle paths also reject hooked mints.
- The modified CPMM and CLMM programs have passed host unit tests; both modified Raydium SBF artifacts have built and loaded as executable accounts in the root ProgramTest runtime. The modified CLMM SBF build exits successfully but emits maximum-frame-size diagnostics in upstream `ObservationState` and `TickArrayState` deserializers. The Raydium ProgramTest so far is only an artifact-loading smoke test; it has not invoked a swap instruction or demonstrated CPI account forwarding. This is not a deployment certification.
- The reviewed LaunchLab public SDK exposes instruction layouts but not the deployed handler implementation. The SDK repository is GPL-3.0 and is not vendored. Migration and fee/vesting transfer behavior remain unverified and unpatched.
- ProgramTest can load the versioned Raydium SBF artifacts, but the handlers and SDK framing builders have not yet been exercised together in a swap transaction; successful artifact loading, unit tests, and SBF compilation do not prove live CPMM or CLMM swap behavior.
- Transfer surfaces beyond CPMM `swap_base_input_v2` and CLMM `swap_v3` have not been made hook-compatible; current helper paths reject hooks and LaunchLab remains blocked.
- Arbitrary hook program upgrades, malicious extra-account requirements, v0/v1 message-size limits, compute budgets, and full LaunchLab lifecycle rollback have not been demonstrated by these tests.

Do not advertise any of those surfaces as hook-compatible until the relevant upstream source is available, patched with explicit per-transfer account framing, and covered by runtime tests. No transaction was submitted to a public cluster.
