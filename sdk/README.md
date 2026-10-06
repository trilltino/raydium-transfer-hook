# SDK account resolution

`crates/transfer-hook-sdk` contains a local resolver abstraction. It validates typed mint/list views, refreshes both for every transfer, delegates PDA derivation and TLV seed resolution to `TransferHookAccountSource`, and returns one account range per transfer.

This is not yet a production Solana SDK: the workspace does not depend on the SPL resolver or RPC client, and the included test source uses fake deterministic addresses. A production provider must use the pinned SPL interface, validate raw account data before decoding, and append account slices only to a Raydium instruction whose parser defines explicit transfer boundaries. See [`../docs/account-resolution.md`](../docs/account-resolution.md).
