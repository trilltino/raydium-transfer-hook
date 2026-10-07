# Hook template registry

Optional, permissionless metadata for Transfer Hook templates: **who published a descriptor for
which hook program and template**. Hashes and keys only; the manifest itself lives off-chain.

> **A descriptor is never permission.** Nothing in Raydium, the SDK or this repository requires a
> hook to be described, and a descriptor says nothing about whether a hook is allowed, safe, tested
> or audited. Anyone can publish one for any program, including one they did not write. This
> program has no owner, no allowlist and no instruction only a registry operator can call.

The off-chain half (manifests, ids, what a reader may conclude) is
[`crates/hook-template-sdk`](../../crates/hook-template-sdk).

## The descriptor

One account per `(hook program, template, publisher)` at the PDA
`["hook-template", hook_program, template_id, publisher]`, 209 bytes:

| Field | Meaning |
|---|---|
| `hook_program` | the program described |
| `template_id` | `SHA-256` of the canonical template manifest: content-derived, so the same manifest always has the same id |
| `manifest_hash` | `SHA-256` of the manifest document as published, to check a copy you were handed |
| `template_authority` | the publisher: the only one who can update or close it |
| `flags` | the publisher's own words; **never evidence** of testing, an audit or approval |

### Why the publisher is in the address

Publication is permissionless. If the address were only `(hook_program, template_id)`, the first
publisher would own it for everyone: a squatter could take the address of a template they did not
write, and the real author could not publish there. With the publisher in the seeds, an account at
an address was created by that publisher and nobody else, and each publisher has their own
descriptor. (The seeds in the original specification omitted the publisher; this is the deliberate
change, for exactly that reason.) To find every descriptor for a template, filter this program's
accounts on the `hook_program` and `template_id` fields.

## What a reader may conclude

The SDK's `assess` derives only one stronger claim from a descriptor: **author-verified**, when the
publisher is the hook program's current upgrade authority (whoever can replace its code). It never
believes the descriptor's flags. "Repository-tested" and "audited" are facts the reader brings
from sources of their own; a descriptor cannot prove them.

## Instructions

| Instruction | Who | What |
|---|---|---|
| `Publish` | anyone | create their own descriptor. The hook must be an executable program; nothing else is checked. |
| `Update` | the descriptor's authority | change `manifest_hash` and `flags` (the template id never changes) |
| `Close` | the descriptor's authority | delete it and take the rent back |

## Errors

| Code | Name | When |
|---|---|---|
| `0xF001` | `InvalidInstruction` | malformed instruction |
| `0xF002` | `InvalidDescriptor` | malformed account, or not at the address its contents imply |
| `0xF003` | `HookProgramNotExecutable` | the account to describe is not a program |
| `0xF004` | `NotTemplateAuthority` | someone other than the publisher tried to update or close |
| `0x8001..` | `hook_kit::KitError` | `0x8006` `AlreadyInitialized`: this publisher already published this template |

## Run it

```bash
cargo test -p hook-template-registry
cargo build-sbf --manifest-path programs/hook-template-registry/Cargo.toml --sbf-out-dir target/integration-sbf
```

## Honest limits

* **It is a side table.** A hook works with no descriptor at all, and a descriptor can exist for a
  hook that is malicious. Do not treat a description as a safety signal.
* **Anyone can describe anyone's program.** A stranger's descriptor for your hook is
  community-published, not you speaking. Only a descriptor published by the hook's upgrade
  authority derives to author-verified.
* **An immutable hook cannot be author-verified through the registry,** because there is no upgrade
  authority to match. Say so in your own README.
* **Flags are free text.** They are a convenience for the publisher and prove nothing.
