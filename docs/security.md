# Security

Supporting the standard interface is compatibility plumbing, not endorsement of a hook program. A
hook is an untrusted program and can refuse any transfer, including deliberately.

## What a platform and its users must account for

* A hook can reject transfers, including sells, withdrawals or migration.
* The hook program may be upgradeable: whoever holds its upgrade authority can replace the rule for
  every mint that uses it.
* The mint's transfer-hook authority may change the configured program.
* The validation `ExtraAccountMetaList` may change independently.
* Hooks can add compute, accounts, writable contention, setup costs or user-specific failure modes.
* Some hook rules may require routing or setup that generic aggregators do not support.

Resolve accounts against sufficiently fresh mint and validation-list state, and treat account data as
untrusted: validate ownership, lengths and expected formats before decoding it.

## Transport readiness is not business readiness

The two are kept apart in the API and in what is printed.

| | Transport readiness | Business readiness |
|---|---|---|
| Question | Can a transfer reach the hook at all? | Is the hook worth trusting? |
| Examples | the mint is Token-2022 and points at the expected program; the program exists and is executable; the validation list exists, is owned by the program and has the Execute shape | the hook's own settings are initialised correctly; the rule is sensible; the program cannot freeze transfers on purpose; who controls the settings |
| Who can answer it | tooling: `raydium_hook_driver::inspect_readiness`, `raydium-hook inspect` | a person reading the hook, or an audit |

Passing every transport check says nothing about the second column.

## What the SDK checks, and what it cannot

For every transfer the SDK re-fetches the mint and the validation list and checks transport
correctness: the mint points at the expected program, the program exists, is executable and is owned
by an allowed loader, the validation list is owned by it and parses, and the resolved accounts carry
no unexpected privileges (a resolved extra that is writable or a signer is refused unless the
integrator named it). It rejects mismatched owners, keys, mint association, truncated layouts and
invalid Execute-list markers, and slices that share a key with different privileges.

It **cannot** judge a hook's economics, who can change its settings, or whether its program can be
upgraded. Its source trait receives typed state and delegates raw TLV parsing and PDA resolution to
the official SPL helpers; that boundary is not a substitute for validating RPC responses in
production.

## Who holds which power

Display these separately; do not collapse them into one "owner".

| Power | Where to read it |
|---|---|
| Replace the hook program's code (upgrade authority) | `inspect_readiness` → `program.upgrade`; `raydium-hook inspect` |
| Re-point the mint at a different hook | the mint's TransferHook extension authority (`hook_authority`; `None` once revoked) |
| Change the hook's per-mint settings | the hook's own config authority; each template's README says who |
| Publish metadata about a template | the descriptor's `template_authority` in the template registry (never implies approval) |

## Malicious-hook tests

Each scenario a hostile or broken hook can present, and the test that covers it. "Flow" means the
Raydium flows in `tests/program-test` and `tests/third-party-hook`, which assert exact error codes
and that every balance is unchanged after a refusal.

| Scenario | Covered by |
|---|---|
| A hook that always rejects | Flows: every refusal rolls back the whole swap, with the hook's own code, in each direction. The generic provider's refusals in `tests/third-party-hook`. |
| A hook that rejects only the output transfer | Flows: the hooked-token-out refusals of `reference-hook`, `fair-launch`, `anti-bundle`. |
| A hook requiring many accounts | SDK: `a_hook_with_a_large_account_list_resolves_every_extra_in_order` (48 extras), `two_large_slices_in_one_swap_stay_separate_and_are_framed_by_their_full_lengths`, `cpmm_rejects_slices_longer_than_the_u16_framing_limit`. |
| A malformed validation list | SDK: `error_validation_list_malformed_variants`, `error_missing_validation_list`. |
| A validation list with the wrong owner | SDK: `error_invalid_validation_list_owner`. |
| The wrong hook executable | SDK: `error_unexpected_hook_program`, `error_hook_program_not_executable`, `error_hook_program_bad_loader`, `error_hook_program_invalid_for_token_and_raydium_programs`. |
| The hook program changed between resolving and sending | SDK: `verify_unchanged_detects_a_program_upgrade`, `verify_unchanged_detects_a_changed_validation_list`, `verify_unchanged_detects_repointed_removed_and_reauthorized_mints`. |
| An unexpected writable extra | SDK: `privilege_policy_rejects_signers_and_writables_by_default`, `a_writable_extra_hidden_in_a_large_list_is_still_refused_and_named`. |
| An unexpected signer request | SDK: `privilege_policy_rejects_signers_and_writables_by_default`, `the_resolved_tail_is_readonly_and_a_hostile_extra_naming_the_hook_is_policed`. |
| Duplicate or aliased accounts across the two legs | SDK: `cpmm_never_merges_or_dedups_slices_that_share_accounts`, `cpmm_rejects_a_key_shared_by_both_slices_with_different_flags`, `cpmm_rejects_slices_that_escalate_shared_accounts`. |
| A hook called directly, not by Token-2022 | Every template and program: `a_direct_execute_call_is_refused` (the `transferring` flag). |
| A hook whose state changes between runs | SDK: `every_resolution_reads_the_chain_afresh`. |
| The hook authority revoked or replaced | SDK: `error_hook_authority_violation`. |

Not covered: a hook that consumes unbounded compute only on some inputs (a timeout in the runtime,
not something the SDK can see before sending; simulate first), and a hook that behaves differently
in simulation than on-chain (it can read the clock and the slot).

## For a hook author

Say who holds your program's upgrade authority, or revoke it. Say what your rule does not stop. Each
example README ends with such a list; for instance a hook never sees a burn, and the priority-fee
check sees the fee a legacy or v0 transaction declares, not a tip paid to a block builder, and does
not work on v1 transactions at all.

## Platform policy models (not on-chain)

`hook-policy-model` describes how a platform could select a hook. It is a design label, not a
deployed access control.

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
A production hook must enforce authority and timelock proofs on-chain.

### The settings as bytes, and the identity lock

`PlatformHookOverlay` in `hook-policy-model` is a 180-byte layout for these settings (hook program,
policy, authority policy, version, flags, 144 reserved bytes) that an all-zero region decodes as "no
hook", so an existing platform account whose padding was never written keeps its meaning. After the
first hooked mint is created the **hook program identity locks**: the same program is still
accepted, a different one or clearing it is refused. A platform wanting a different hook engine
creates another platform account.

This is a **model**. The layout assumes a 180-byte padding region that can be overlaid without
resizing the account (the reviewed public CPI state of LaunchLab's `PlatformConfig` exposes 180
bytes of padding); nothing has been checked against LaunchLab's deployed program, whose handler is
not public, and no on-chain account is read or written. Before storing this anywhere, inspect the
exact serialization, padding, IDL and upgrade compatibility, and do not assume unused bytes can be
repurposed safely.
