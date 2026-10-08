# Transfer Hook starter

A deployable Token-2022 Transfer Hook with the hard parts done. You change one file,
[`src/rule.rs`](src/rule.rs). The default rule is the simplest useful one:

```text
if amount > max_transfer { reject } else { allow }
```

The starter is standalone: it has its own `[workspace]` table and no path dependencies, so it
builds anywhere you copy it. Using it is optional; a hook that shares no code with it works the same
way, because nothing in Token-2022 checks where a hook came from.

## What is already done

* The SPL `Execute` entrypoint and the check that rejects calls that did not come from a Token-2022
  transfer (the `transferring` flag, on both token accounts).
* The canonical validation list (`ExtraAccountMetaList`), created atomically with the per-mint config
  by one `InitializeHook` instruction.
* A per-mint config account (`["hook-config", mint]`), versioned, with four authority modes
  (extension authority, mint authority, explicit, immutable), `UpdateConfig` and `SetConfigAuthority`.
* Typed error codes (`HookError`, from `0x7001`) so callers can tell your hook caused a refusal.
* Tests that run against the real Token-2022 processor.
* `examples/devnet.rs`: creates a hooked mint, initialises the hook and proves allow/reject on a
  real cluster. `scripts/deploy.sh` runs it after deploying.

## The flow

From the repository root (Git Bash on Windows):

1. **Copy it.** `cp -r starter my-hook`. Renaming the crate is optional; if you want to, run
   `sed -i 's/transfer_hook_starter/my_hook/g; s/transfer-hook-starter/my-hook/g' my-hook/Cargo.toml my-hook/tests/*.rs my-hook/examples/*.rs`
   (the tests and the example refer to the crate by name).
2. **Edit the rule.** In `my-hook/src/rule.rs` change `validate_params` (what a creator may
   configure) and `check_transfer` (allow or refuse each transfer; it receives a `TransferContext`
   with amount, source, destination, mint and authority).
3. **Test it.** `cd my-hook && cargo test`. The tests expect the default max-transfer rule. Update
   the ones that exercise it when you change the rule; keep the plumbing tests (direct-call
   rejection, authority, config layout). Against the real SBF build:
   `cargo build-sbf --sbf-out-dir target/deploy && SBF_OUT_DIR=$PWD/target/deploy cargo test`.
4. **Build.** `scripts/build.sh my-hook` writes `my-hook/target/deploy/<name>.so` and, the first
   time, `<name>-keypair.json`: your program id.
5. **Deploy and prove it.** `scripts/deploy.sh my-hook --cluster devnet`. If you changed the rule's
   parameters, update the `InitializeHookArgs` in `examples/devnet.rs` to match.
6. **Decide who holds the upgrade authority.** It can replace your rule for every token that uses
   it. Say so publicly, or revoke it: `solana program set-upgrade-authority <ID> --final`.

## Things your rule must respect

* Hooks are untrusted programs and can refuse any transfer. Keep the rule bounded and deterministic.
* Every extra account you add to the validation list is added to every transfer of the mint. Stay
  well under `hook_kit::PRACTICAL_EXTRA_ACCOUNTS`.
* Anything the hook writes must be a writable extra, and then all transfers of the mint contend for
  it. See [`DESIGN.md`](../DESIGN.md).
* Hooks cannot move the tokens being transferred and never run for burns.
* Token-2022 moves the tokens before it calls the hook: balances you read are post-transfer.
* Raydium and other venues only work with your hook if they forward the hook's accounts. Check the
  venue's documentation.

To contribute a rule as a template, see [`CONTRIBUTING.md`](../CONTRIBUTING.md).
