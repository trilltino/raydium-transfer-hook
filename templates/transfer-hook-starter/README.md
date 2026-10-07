# Transfer Hook starter

A deployable Token-2022 Transfer Hook with the hard parts already done. You change one file,
[`src/rule.rs`](src/rule.rs).

Using this template is optional. A hook that shares no code with it works the same way: nothing
in Raydium, in this repository's SDK, or in any registry asks whether a hook came from here. The
repository ships a second, unrelated hook (`programs/arbitrary-test-hook`) to prove that.

## What is already done

- The SPL `Execute` entrypoint and the check that rejects calls that did not come from a
  Token-2022 transfer (the `transferring` flag, checked on both token accounts).
- The canonical validation list (`ExtraAccountMetaList`), created atomically with the per-mint
  config by one `InitializeHook` instruction.
- A per-mint config account (`["hook-config", mint]`), versioned, with authority modes
  (extension authority, mint authority, explicit, immutable), `UpdateConfig` and
  `SetConfigAuthority`.
- Typed error codes (`HookError`, starting at `0x7001`) so integrators can tell your hook caused a
  refusal.
- Tests that run against the real Token-2022 processor.

## The flow

1. **Copy the starter** (from the repository root).
   ```sh
   cp -r templates/transfer-hook-starter my-hook     # PowerShell: Copy-Item -Recurse templates\transfer-hook-starter my-hook
   ```
2. **Edit the rule.** Open `src/rule.rs`. Change `validate_params` (what a creator may configure)
   and `check_transfer` (allow or refuse each transfer). Rename the crate in `Cargo.toml`.
3. **Test it.**
   ```sh
   cargo test --manifest-path my-hook/Cargo.toml
   ```
   The tests expect the default max-transfer rule. Update the ones that exercise it when you
   change the rule; keep the plumbing tests (direct-call rejection, authority, config layout).
   To run them against the real SBF build: `cargo build-sbf` in `my-hook`, then
   `SBF_OUT_DIR=$PWD/target/deploy cargo test` (PowerShell: `$env:SBF_OUT_DIR = (Resolve-Path target\deploy).Path`).
4. **Describe its setup in `setup.json`.** The stack never links your crate: it initialises your
   hook for a mint from this file (the `InitializeHook` bytes, which accounts, and which swaps
   your rule must refuse with which error code). `tests/setup_json.rs` checks it against the real
   encoding, so it fails if you change the rule's parameters or error code without updating it.
5. **Run it through real Raydium pools on a local validator** (from the repository root; no keys):
   ```sh
   cargo xtask localnet build
   cargo xtask localnet validator          # leave running; in another terminal:
   cargo run -p raydium-hook-cli -- e2e --env environments/localnet.json \
     --keypair tests/fixtures/localnet/admin.json --keys target/localnet/keys \
     --amm all --hook-dir my-hook
   ```
   This builds and deploys your hook, creates a hooked mint and real CPMM and CLMM pools, swaps
   both ways, makes your hook refuse, checks the rollback, and prints a PASS/FAIL table.
6. **Deploy to devnet.** The same `e2e --hook-dir` against an environment file for devnet and a
   funded keypair; the hook-aware Raydium programs must exist there (ours are listed in
   `environments/devnet.json`, see `docs/forking.md` for your own). Or deploy by hand:
   ```sh
   solana program deploy target/deploy/transfer_hook_starter.so --url devnet \
     --program-id <PROGRAM_KEYPAIR.json> --upgrade-authority <KEYPAIR.json>
   ```
   Decide who holds the upgrade authority. It can replace your rule for every token, so say so
   publicly or revoke it.

## Things your rule must respect

- Hooks are untrusted programs and can refuse any transfer. Keep the rule bounded and
  deterministic.
- Every extra account you add to the validation list is added to every hooked transfer's
  transaction. Raydium swaps carry two legs.
- Hooks cannot move the tokens being transferred.
- Liquidity deposits and withdrawals on Raydium do not accept hooked mints yet, so enable the hook
  after a pool has its liquidity.
