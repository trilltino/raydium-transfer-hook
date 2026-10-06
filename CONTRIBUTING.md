# Contributing

Keep the first implementation small and evidence-based.

1. Identify the exact upstream Raydium instruction and transfer helper being changed.
2. Verify the Token-2022 and Transfer Hook Interface APIs against their current source.
3. Preserve existing V1 behavior, discriminators, and fixed account layouts.
4. Add a focused test for hooked and non-hooked transfers, including missing and stale extra accounts.
5. Record verified behavior separately from assumptions in the architecture notes.
6. Run `cargo test --workspace` and any program-specific build and integration tests.

Do not claim an integration works based only on the policy model. Do not commit generated keypairs, RPC credentials, or mainnet transaction artifacts.

