# CLMM flow model

`src/lib.rs` plans a separate tick/bitmap prefix and hook-transfer tail for two swap legs.

The reviewed upstream `SwapV2` parser scans remaining accounts as tick arrays/bitmap extension by data length and stops at the first other account. Its transfer helper does not forward hook accounts. The modeled partition is not a live SwapV2 extension; a real hooked entrypoint needs an explicit boundary/new discriminator and program tests.
