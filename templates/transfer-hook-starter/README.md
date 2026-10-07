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

1. **Copy the starter.**
   ```powershell
   Copy-Item -Recurse templates\transfer-hook-starter my-hook
   cd my-hook
   ```
2. **Edit the rule.** Open `src/rule.rs`. Change `validate_params` (what a creator may configure)
   and `check_transfer` (allow or refuse each transfer). Rename the crate in `Cargo.toml`.
3. **Test it.**
   ```powershell
   cargo test
   ```
   The tests expect the default max-transfer rule. Update the ones that exercise it when you
   change the rule; keep the plumbing tests (direct-call rejection, authority, config layout).
4. **Build the program.**
   ```powershell
   cargo build-sbf
   ```
   The `.so` is written to `target/deploy/`. Run the tests again against the real build:
   ```powershell
   $env:SBF_OUT_DIR = (Resolve-Path target\deploy).Path
   cargo test
   ```
5. **Deploy** (devnet):
   ```powershell
   solana program deploy target\deploy\transfer_hook_starter.so --url devnet `
     --program-id <PROGRAM_KEYPAIR.json> --upgrade-authority <KEYPAIR.json>
   ```
   Decide who holds the upgrade authority. It can replace your rule for every token, so say so
   publicly or revoke it.
6. **Create a hooked mint.** A Token-2022 mint with the `TransferHook` extension whose program id
   is your deployed program and whose authority is the key that will initialise the hook.
7. **Initialise the hook for that mint** with `initialize_hook_instruction` (see
   `tests/token_2022_transfer.rs` for a worked example). Pick the authority mode here.
8. **Run it through Raydium.** Resolve each swap leg's hook accounts with the SDK in
   `crates/transfer-hook-sdk` and use the hook-aware CPMM `swap_base_input_v2` or CLMM
   `swap_v3` instructions on a build that includes them (see `docs/integration-devnet.md`).
   You do not edit the SDK or the Raydium adapters for your hook.

## Things your rule must respect

- Hooks are untrusted programs and can refuse any transfer. Keep the rule bounded and
  deterministic.
- Every extra account you add to the validation list is added to every hooked transfer's
  transaction. Raydium swaps carry two legs.
- Hooks cannot move the tokens being transferred.
- Liquidity deposits and withdrawals on Raydium do not accept hooked mints yet, so enable the hook
  after a pool has its liquidity.
