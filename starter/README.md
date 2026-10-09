# Transfer Hook starter

A deployable Token-2022 Transfer Hook with the hard parts done. **Start in
[`src/rule.rs`](src/rule.rs).** The default rule is the simplest useful one:

```text
if amount > max_transfer { reject } else { allow }
```

The starter is standalone: it has its own `[workspace]` table and no path dependencies, so it
builds anywhere you copy it. Using it is optional; a hook that shares no code with it works the same
way, because nothing in Token-2022 checks where a hook came from.

## What is already done

* The SPL `Execute` entrypoint and the check that rejects calls that did not come from a Token-2022
  transfer (the `transferring` flag, on both token accounts).
* The validation list (`ExtraAccountMetaList`), created atomically with the per-mint config by one
  `InitializeHook` instruction, and checked byte for byte against `CANONICAL_VALIDATION_LIST` on every
  transfer.
* A per-mint config account (`["hook-config", mint]`), versioned, written once by the mint's Transfer
  Hook authority. It cannot be changed afterwards (see below if your rule needs that).
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
3. **Test it.** `cd my-hook && cargo test --locked`. The tests expect the default max-transfer rule.
   Update the ones that exercise it when you change the rule; keep the plumbing tests (direct-call
   rejection, authority, config layout). Against the real SBF build:
   `cargo build-sbf --sbf-out-dir target/deploy && SBF_OUT_DIR=$PWD/target/deploy cargo test --locked`.
4. **Deploy and prove it.** `scripts/deploy.sh my-hook` builds with `cargo build-sbf` (writing
   `my-hook/target/deploy/<name>.so` and, the first time, `<name>-keypair.json`: your program id's
   private key, never commit it), deploys to devnet, then runs `examples/devnet.rs`. Options for the
   example go after `--`, for example `scripts/deploy.sh my-hook -- --limit 1000`. If you changed the
   rule's parameters, update the `InitializeHookArgs` in that example to match.
5. **Decide who holds the upgrade authority.** It can replace your rule for every token that uses
   it. Say so publicly, or revoke it: `solana program set-upgrade-authority <ID> --final`.

## When `rule.rs` is not enough

Simple rules that fit the existing config (one `u64` param today, up to 256 bytes) may only need
`rule.rs`, its error variants and its tests. Rules that introduce new configuration, state or extra
accounts must also update the corresponding plumbing and tests:

| The rule needs | Also change |
|---|---|
| different settings | `validate_params`, the params helpers in `config.rs`, `InitializeHookArgs` in `instruction.rs`, `examples/devnet.rs` |
| another extra account | `CANONICAL_VALIDATION_LIST` and `VALIDATION_LIST_LEN` in `constants.rs`, `config_extra_account_meta` in `pda.rs`, the account count and checks in `processor/execute.rs` |
| state the hook writes | the above with the account **writable** (every transfer of the mint then contends for it), plus its layout and initialisation |
| settings that can change | an update instruction (next section) |

Those files carry the security checks. [`AGENTS.md`](../AGENTS.md) asks AI agents to stop and explain
before touching them; do the same for a human reviewer, and keep a test for every check.

## If the settings must be changeable

The starter sets its config once and never changes it, like the templates. If your rule needs
mutable settings or other authority models, add an `UpdateConfig` instruction that checks the signer
against an authority you choose and a sequence number against replays. A worked version with four
authority modes is in git history (tag `pre-community-hook-kit`, `templates/transfer-hook-starter`).
Add the matching tests: unauthorized update refused, stale sequence refused.

## Things your rule must respect

* Hooks are untrusted programs and can refuse any transfer. Keep the rule bounded and deterministic.
* Every extra account you add to the validation list is added to every transfer of the mint. Stay
  well under ten PDA-derived extras (`hook_kit::PRACTICAL_EXTRA_ACCOUNTS`; see DESIGN.md).
* Anything the hook writes must be a writable extra, and then all transfers of the mint contend for
  it. See [`DESIGN.md`](../DESIGN.md).
* `Execute` gets the transfer accounts read-only and without the transfer authority's signature, so
  the hook cannot re-spend the transferred tokens through it. Hooks never run for burns or mints.
* Token-2022 moves the tokens before it calls the hook: balances you read are post-transfer.
* Raydium and other venues only work with your hook if they forward the hook's accounts. Check the
  venue's documentation.

To contribute a rule as a template, see [`CONTRIBUTING.md`](../CONTRIBUTING.md).
