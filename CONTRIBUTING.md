# Contributing

Keep changes small and evidence-based.

1. Identify the exact upstream Raydium instruction and transfer helper involved. Raydium source is
   never added to this repository: changes to it go to the external forks, and the new commit is
   pinned in `upstream.lock.toml` and recorded in `docs/source-lock.md`.
2. Verify Token-2022 and Transfer Hook Interface behavior against their current source.
3. Preserve existing V1 behavior, discriminators and fixed account layouts.
4. Add a focused test for hooked and non-hooked transfers, including missing and stale extra
   accounts. A hook rule change needs a test with the exact error code.
5. Record support only in `docs/transfer-surface-matrix.md`, with the strongest evidence you
   actually have: unit-tested, runtime-verified in-process, or verified on devnet.
6. Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, and `cargo xtask upstream verify`. When you touch a hook program, the
   framers or the flows, also run the runtime tests: `cargo xtask localnet build`, the ignored tests
   (see `tests/README.md`) and `cargo xtask localnet e2e --skip-build`. CI runs all of these.
7. Do not claim an integration works from a model alone. Do not commit keypairs, RPC credentials
   or mainnet transaction artifacts (`.keys/` and `reference/` are git-ignored). The only tracked
   keys are the throwaway localnet ones in `tests/fixtures/localnet`, which must never be funded on
   a public cluster.
