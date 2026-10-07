# Trust model

Supporting the standard interface is compatibility plumbing, not endorsement of a hook program. A
hook is an untrusted program and can refuse any transfer, including deliberately.

## What a platform and its users must account for

* A hook can reject transfers, including sells, withdrawals or migration.
* The hook program may be upgradeable.
* The mint's transfer-hook authority may change the configured program.
* The validation `ExtraAccountMetaList` may change independently.
* Hooks can add compute, accounts, writable contention, setup costs or user-specific failure modes.
* Some hook rules may require routing or setup that generic aggregators do not support.

Resolve accounts against sufficiently fresh mint and validation-list state, and treat account data as
untrusted: validate ownership, lengths and expected formats before decoding it.

## What the SDK checks, and what it cannot

For every transfer the SDK re-fetches the mint and the validation list and checks transport
correctness: the mint points at the expected program, the program exists, is executable and is owned
by an allowed loader, the validation list is owned by it and parses, and the resolved accounts carry
no unexpected privileges (a resolved extra that is writable or a signer is refused unless the
integrator named it). It rejects mismatched owners, keys, mint association, truncated layouts and
invalid Execute-list markers.

It **cannot** judge a hook's economics, who can change its settings, or whether its program can be
upgraded. Its source trait receives typed state and delegates raw TLV parsing and PDA resolution to
the official SPL helpers; that boundary is not a substitute for validating RPC responses in
production.

## For a hook author

Say who holds your program's upgrade authority, or revoke it. Say what your rule does not stop. Each
example README ends with such a list; for instance a hook never sees a burn, and the priority-fee
check sees the fee a transaction declares, not a tip paid to a block builder.

## Platform policy models (not on-chain)

`hook-policy-model` and `reference-hook-model` describe how a platform could select a hook. They are
design labels, not deployed access controls.

A platform selects at most one hook program. A launch can choose a supported preset or parameters
within that engine; it cannot replace the platform-selected program.

| Policy | Meaning |
|---|---|
| **Disabled** | no launch can request a hook preset |
| **Optional** | the platform may configure an engine; a launch may use it or launch without one |
| **Mandatory** | an engine must be configured and is selected for every launch |

| Authority policy | Meaning in the model |
|---|---|
| **PlatformRetained** | the configured platform authority can update rules; changing the mint, hook program or authority policy is rejected |
| **ImmutableAtLaunch** | engine reconfiguration is rejected after initialisation |
| **GovernedTimelock** | the model accepts an authority token standing for timelock approval. It does not verify signatures, delay, governance execution or on-chain account constraints |

Only the first two authority rules are simple state checks; the timelock token proves no real delay.
A production hook must enforce authority and timelock proofs on-chain. Before storing a policy in a
Raydium account, inspect the exact serialization, padding, IDL and upgrade compatibility, and do not
assume unused bytes can be repurposed safely.
