# hook-kit

The shared plumbing every Token-2022 Transfer Hook needs, so a hook is mostly its rule.

**Where it fits.** This repository is a kit for going from a token business-logic idea to a
deployed, reviewable Transfer Hook: `rule.rs` -> test -> deploy -> contribute. `hook-kit` is the part
you should not have to write or re-audit while doing that. The three [`templates/`](../templates) are
built on it. The [`starter/`](../starter) deliberately carries its own copy of this plumbing so it
still builds when you copy it out of the repository.

## What it handles

| Module | What it does for a hook |
|---|---|
| `execute` | `execute_prelude`: every check a hook must make before its own rule (called by Token-2022, both token accounts flagged `transferring`, the validation list is the mint's, account count). `parse_execute_amount` |
| `mint` | read the hooked mint; `require_hook_program`, `require_extension_authority` (who may set the hook up), `require_mint_authority_revoked` |
| `token` | read a token account: owner, mint, post-transfer balance |
| `accounts` | create PDAs (surviving a pre-funded address) and the canonical validation list (`ExtraAccountMetaList`) |
| `error` | `KitError`, codes from `0x8001`, so a caller can tell shared-check failures from your rule's errors |
| `testing` | (feature `test-support`) a Token-2022 mint with your hook, funded accounts, transfers that resolve the hook's accounts the way a wallet does, `assert_custom_error`, a settable clock |

Two facts a rule depends on, both in [`DESIGN.md`](../DESIGN.md): Token-2022 moves the tokens
**before** it calls the hook, so balances are post-transfer; and a hook cannot spend the transfer
authority.

## Using it

A template depends on it by path and enables the test helpers in dev-dependencies:

```toml
[dependencies]
hook-kit = { path = "../../hook-kit" }

[dev-dependencies]
hook-kit = { path = "../../hook-kit", features = ["test-support"] }
```

`World::transfer_ix` panics if your hook declares a writable or signer extra account you did not
list in `allow_writable`. That is on purpose: every writable account is a contention decision, and a
template must name it.

## Changing it

Rarely. A bug here affects every template, so a change needs a test and all templates' tests must
pass (`cargo test --workspace` from the repository root). See [`AGENTS.md`](../AGENTS.md).
