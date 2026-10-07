# Account resolution

## How a leg is resolved

`transfer_hook_sdk::resolve_leg` resolves one transfer, fresh, against the chain state its fetcher
returns. It is built on the official `spl-transfer-hook-interface` off-chain helper; the crate does
not reimplement the TLV or seed grammar and does not derive any non-canonical address.

1. Read the mint. It must be owned by Token-2022 and unpack as a mint.
2. If the mint has no TransferHook extension or no hook program, the leg has no slice.
3. Check the hook program: it must exist, be executable, be owned by an allowed loader, and not be
   Token-2022 or a Raydium program. `ResolveOptions` can pin the exact program and the required
   extension authority.
4. Derive the canonical validation address (`get_extra_account_metas_address`), fetch it, and
   require that it is owned by the hook program and parses as an `ExtraAccountMetaList` for Execute.
5. Resolve the list for this transfer's exact source, mint, destination, authority and amount.
6. Apply the privilege policy: a resolved extra that is a signer or writable is refused unless the
   integrator named it. The hook program and validation list must be read-only non-signers.
7. Return a `LegHook`: `[resolved extras..., hook program, validation list]` and a fingerprint of the
   mint, program and list it was resolved against.

Nothing is cached. `LegHook::verify_unchanged` re-reads the three accounts so a caller can check
that nothing changed between resolving and signing. Errors are structured, `Clone + PartialEq`, and
name the leg (`LegError { leg, mint, source }`).

## Resolving several legs

`resolve_legs` resolves on a private scratch instruction, so a failure on the second leg leaves the
caller's instruction untouched. Resolution never mutates a caller's instruction; only the framers
do, after validating every leg.

## Per-transfer slices

Slices are appended input-then-output and are never flattened, merged, deduplicated or reordered.
The framers (`frame_cpmm_swap_base_input_v2`, `frame_clmm_swap_v3`, and the `*_or_passthrough`
forms that leave a swap with no hooked leg as the byte-identical V1) take resolved `LegHook`s, never
raw account metas, re-derive each leg's validation address, and reject trailing accounts and
privilege conflicts. See [versioning](versioning.md) for the instruction layouts.

## Source contract

The SPL off-chain helper (reviewed at `ec7063291e968f4b0064e4df0324ff49dcf320df`) derives the
validation PDA from the `extra-account-metas` seed and mint, resolves the list against an Execute
instruction (`[105, 37, 101, 197, 75, 251, 102, 26]` plus the amount as little-endian `u64`), and
appends the resolved metas followed by the hook program id and the validation list. Token-2022
reads the hook program from the mint and invokes it with the transfer accounts plus the
caller-supplied additional accounts, so a program that performs the transfer must forward each
leg's slice to its own transfer CPI. That is what the hook-aware Raydium builds do.
