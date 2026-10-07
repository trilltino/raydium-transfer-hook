# Tests

```powershell
cargo test --workspace
```

runs everything that does not need built SBF artifacts. Tests that do need them are `#[ignore]`
and fail loudly, not silently, when an artifact or key is missing.

| Where | What it proves | Needs |
|---|---|---|
| `programs/reference-hook-onchain/tests/hook_program.rs` | Every init/update/authority/execute rule of the reference hook, with exact error codes | nothing (SBF run when `SBF_OUT_DIR` is set) |
| `programs/reference-hook-onchain/tests/token_2022_transfer.rs` | The real Token-2022 transfer calls the hook; a refused transfer rolls back | nothing |
| `programs/reference-hook-onchain/tests/{cpmm_swap_base_input_v2,clmm_swap_v3}_runtime.rs` | Hooked swaps through the real CPMM / CLMM SBF programs: hook runs once per hooked leg, refusals originate in the hook, pool state is untouched | `SBF_OUT_DIR` with `raydium_cp_swap.so` / `raydium_clmm.so` and the hook `.so` |
| `programs/arbitrary-test-hook/tests` | The unrelated hook: init, N+2 resolution through the unchanged SDK, state mutation, rollback of balances and its counter | nothing |
| `templates/{creator-commitment,fair-launch,loyalty-rewards}/tests` | Each example hook inside real Token-2022 transfers: every boundary of the rule, exact error codes, rollback after each refusal, setup validation, direct-call refusal | nothing (SBF run when `SBF_OUT_DIR` is set) |
| `crates/hook-kit` | Shared hook plumbing compiles and is exercised through the templates' tests | nothing |
| `tests/program-test/tests/local_flows.rs` | The ten combinations (CPMM, CLMM) x (reference, arbitrary, creator-commitment, fair-launch, loyalty-rewards), with real admin instructions, against the exact artifacts deployed to devnet; each hook's refusals and follow-up steps (wait out a window, fund and claim) | `target/integration-sbf` artifacts and `.keys/` |
| `crates/transfer-hook-sdk` unit tests | Resolver, framers, V1 goldens | nothing |
| `tests/models` | Model-level checks of policy, SDK and planners against an in-memory chain. Not an execution of any Raydium program despite the name | nothing |

ProgramTest is the real runtime executing real binaries, but it is not a validator process.
