# CPMM planning model (MODEL ONLY)

`src/lib.rs` resolves the two transfer legs of a `swap_base_input` with the SDK (`plan_cpmm_swap_base_input`) and frames them with `frame_cpmm_or_passthrough`. It is not a Raydium CPI and executes nothing.

- Live hooked entrypoint: `swap_base_input_v2`. A swap with no hooked leg stays the byte-identical V1 `swap_base_input`.
- Deposits and withdrawals are rejected by the program for hooked mints, so there is no plan for them.
- Each leg keeps its own `extras.., hook_program, validation_list` slice. Slices are never merged or deduplicated.
- Runtime evidence is `programs/reference-hook-onchain/tests/cpmm_swap_base_input_v2_runtime.rs`; see `docs/source-lock.md` for its recorded status.
