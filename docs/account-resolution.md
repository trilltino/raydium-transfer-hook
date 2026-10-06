# Account resolution

## Resolver behavior

`TransferHookResolver` processes each `TransferContext` separately:

1. Fetch the mint again; verify the requested key, Token-2022 owner, and base mint length.
2. If there is no Transfer Hook program in the typed mint state, return an empty account slice without fetching a validation list.
3. Derive the validation-list PDA for the hook program and mint. A production `TransferHookAccountSource` must use `spl_transfer_hook_interface::get_extra_account_metas_address`; the local test provider uses a deterministic fake key and is not a Solana PDA implementation.
4. Fetch and validate the validation-list key, owner (hook program), mint, minimum TLV header length, and Execute discriminator.
5. Resolve the list against the exact transfer context. A production provider must delegate decoding and seed/account resolution to the SPL TLV account-resolution implementation; this workspace intentionally does not reproduce SPL's seed grammar.
6. Append the resolved additional metas, hook program, and validation-list account in that order, matching the reviewed SPL off-chain helper.

The resolver does not cache mint or validation-list state. `resolve_batch` invokes the provider for each leg and records a separate `Range<usize>` for each transfer. It does not deduplicate repeated accounts or merge privileges.

## Source contract reviewed

The SPL `spl-transfer-hook-interface` off-chain helper (reviewed at commit `ec7063291e968f4b0064e4df0324ff49dcf320df`) derives the validation PDA from the `extra-account-metas` seed and mint, fetches its data, builds an Execute instruction with source/mint/destination/authority and the validation state, asks `ExtraAccountMetaList::add_to_instruction` to resolve additional metas, then appends the resolved metas followed by the hook program id and validation-list account to the caller's instruction. Execute data is the 8-byte SPL discriminator `[105, 37, 101, 197, 75, 251, 102, 26]` followed by the transfer amount as little-endian `u64`.

The Token-2022 transfer processor reads the hook program from the mint extension and invokes it with the current transfer accounts plus caller-supplied additional accounts. Its on-chain helper locates the hook program and validation PDA among those supplied accounts and resolves the TLV list for Execute.

The local provider boundary supplies already-decoded typed views. Production implementations still need to validate account owner/data before decoding, use the exact SPL Execute discriminator and TLV parser, check account executability, handle RPC errors, and produce `AccountMeta` privileges exactly as encoded/resolved.

## Per-transfer slices

Do not flatten transfers into a global union unless the target program defines unambiguous framing and the CPI helper consumes the exact corresponding subset. The CPMM and CLMM reference adapters preserve one range per transfer; CLMM additionally leaves tick/bitmap accounts in a separate prefix.

See [`builder-pseudocode.ts`](builder-pseudocode.ts) for the still-generic client construction sketch and the [ABI matrix](transfer-surface-matrix.md) for why it is not yet a live Raydium builder.
