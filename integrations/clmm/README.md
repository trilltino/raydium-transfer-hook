# CLMM planning model (MODEL ONLY)

`src/lib.rs` resolves the two transfer legs of a swap (`plan_clmm_swap_v3`) and frames them with `frame_clmm_or_passthrough` into `swap_v3`. It is not a Raydium CPI and executes nothing.

- Live hooked entrypoint: `swap_v3`. An unhooked swap stays the byte-identical `swap_v2`.
- Tick arrays and the bitmap extension form a prefix that is only counted (`ticks`, `bitmaps`); the per-leg hook slices follow it.
- No CLMM hooked swap has been executed on a runtime. This crate's tests only check account framing.
