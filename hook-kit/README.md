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
| `execute` | `execute_prelude`: every check a hook must make before its own rule (called by Token-2022, both token accounts flagged `transferring`, read-only fixed accounts, the exact account count, the validation list is the mint's and equals the hook's canonical list byte for byte). `parse_execute_amount` |
| `mint` | read the hooked mint; `require_hook_program`, `require_extension_authority` (who may set the hook up), `require_mint_authority_revoked` |
| `token` | read a token account: owner, mint, post-transfer balance |
| `accounts` | create PDAs (surviving a pre-funded address); build the canonical validation list at compile time (`canonical_list`, `seeded_meta`, `pubkey_meta`) and write it with `create_validation_list` |
| `error` | `KitError`, codes from `0x8001`, so a caller can tell shared-check failures from your rule's errors |
| `testing` | (feature `test-support`) a Token-2022 mint with your hook, funded accounts, transfers that resolve the hook's accounts the way a wallet does, `assert_custom_error`, a settable clock |

Two facts a rule depends on, both in [`DESIGN.md`](../DESIGN.md): Token-2022 moves the tokens
**before** it calls the hook, so balances are post-transfer; and `Execute` gets the transfer accounts
read-only, without the transfer authority's signature, so a hook cannot re-spend them through it.

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
pass (`cargo test --locked --workspace` from the repository root). The starter carries its own copy
of this plumbing: a change to shared security behaviour here means reviewing the same behaviour in
`starter/src/processor/` (see "Starter / hook-kit parity" in [`AGENTS.md`](../AGENTS.md)).

## Canonical validation lists

A template defines one, or a small finite set, of canonical validation-list shapes as constants:

```rust
pub const VALIDATION_LIST: [u8; list_len(1)] = canonical_list([seeded_meta(b"config", 1, false)]);
```

`Initialize` selects the shape that fits the mint's config and writes it with
`create_validation_list`; `execute_prelude` compares the list account with the expected shape byte
for byte on every transfer. Fair Launch, for example, has two shapes (`VALIDATION_LIST`, and
`VALIDATION_LIST_WITH_FEE_CHECK` when the priority-fee check is on) and picks the expected one from
the account count. Comparing bytes is cheaper than resolving the list through the SPL crate on each
`Execute` (in the starter, the same change cut measured `Execute` compute from about 17k to 9k CU,
in-process against the SBF build), and a corrupt list cannot panic anything.

The builders cover the subset of `ExtraAccountMeta` layouts this kit needs: a PDA of the hook
program with seeds `[literal, accounts[index]]` (`seeded_meta`) and a fixed address
(`pubkey_meta`). They do not model every SPL resolver configuration (instruction-data or
account-data seeds, external PDAs). Unit tests pin them to what the SPL crate writes. The extra
accounts themselves are the template's to check; Token-2022 has already resolved their addresses
from the list.
