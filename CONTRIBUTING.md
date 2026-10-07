# Contributing

Keep changes small and evidence-based.

1. Identify the exact upstream Raydium instruction and transfer helper involved. Raydium source is
   never added to this repository: changes to it go to the external forks, and the new commit is
   pinned in `upstream.lock.toml` and recorded in `docs/upstream-sources.md`.
2. Verify Token-2022 and Transfer Hook Interface behavior against their current source.
3. Preserve existing V1 behavior, discriminators and fixed account layouts.
4. Add a focused test for hooked and non-hooked transfers, including missing and stale extra
   accounts. A hook rule change needs a test with the exact error code.
5. Record support only in `docs/raydium-instructions.md`, with the strongest evidence you
   actually have: unit-tested, runtime-verified in-process, or verified on devnet.
6. Run `cargo fmt --all -- --check`, `cargo test --workspace`, and `cargo xtask upstream verify`.
   Run the ignored runtime tests when you touch a hook program or the framers.
7. Do not claim an integration works from a model alone. Do not commit keypairs, RPC credentials
   or mainnet transaction artifacts (`.keys/` and `reference/` are git-ignored).
