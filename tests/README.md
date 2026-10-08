# Tests

```sh
cargo test --workspace
```

runs everything that does not need built SBF artifacts. Tests that do need them are `#[ignore]`
and fail loudly, not silently, when an artifact is missing. Build the artifacts with no keys, then
run them:

```sh
cargo xtask localnet build
cargo test -p program-test-flows -p third-party-hook-acceptance -- --ignored
cargo xtask localnet e2e --skip-build        # the same flows on a real solana-test-validator
```

`RTH_PROFILE=integration` runs the flows against the exact devnet artifacts in
`target/integration-sbf` and the keys in `.keys/` instead.

| Where | What it proves | Needs |
|---|---|---|
| `templates/transfer-hook-starter/tests/hook_program.rs` | Every init/update/authority/execute rule of the starter (the reference hook), with exact error codes | nothing (SBF run when `SBF_OUT_DIR` is set) |
| `templates/transfer-hook-starter/tests/token_2022_transfer.rs` | The real Token-2022 transfer calls the hook; a refused transfer rolls back | nothing |
| `tests/program-test/tests/{cpmm_swap_base_input_v2,clmm_swap_v3}_runtime.rs` | Hooked swaps through the real CPMM / CLMM SBF programs: hook runs once per hooked leg, refusals originate in the hook, pool state is untouched | `SBF_OUT_DIR` with `raydium_cp_swap.so` / `raydium_clmm.so` and the hook `.so` |
| `programs/arbitrary-test-hook/tests` | The unrelated hook: init, N+2 resolution through the unchanged SDK, state mutation, rollback of balances and its counter | nothing |
| `templates/*/tests` | Each example hook inside real Token-2022 transfers: every boundary of the rule, exact error codes, rollback after each refusal, setup validation, direct-call refusal | nothing (SBF run when `SBF_OUT_DIR` is set) |
| `crates/hook-kit` | Shared hook plumbing compiles and is exercised through the templates' tests | nothing |
| `tests/program-test/tests/local_flows.rs` | (CPMM, CLMM) x every hook, plus different hooks per leg, the same hook on both legs and transfer-fee mints, with real admin instructions; each hook's refusals and follow-up steps (wait out a window, fund and claim) | `cargo xtask localnet build` (or `RTH_PROFILE=integration`) |
| `tests/third-party-hook` | A hook known only by its program id and a JSON description, through both AMMs | same |
| `templates/transfer-hook-starter/tests/setup_json.rs` | The starter's `setup.json` matches its real `InitializeHook` encoding and error code | nothing |
| `cargo xtask localnet e2e` | Every hook, and the starter built from source, through both AMMs on a real `solana-test-validator` | the Solana CLI |
| `crates/transfer-hook-sdk` unit tests | Resolver, framers, V1 goldens | nothing |

ProgramTest is the real runtime executing real binaries; `cargo xtask localnet e2e` is a real
validator process driven over RPC. CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml))
runs all of it from a clean checkout.
