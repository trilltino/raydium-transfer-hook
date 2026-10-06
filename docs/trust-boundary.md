# Trust boundary

Supporting the standard interface is compatibility plumbing, not endorsement of a hook program.

A platform and its users must account for these risks:

- A hook can reject transfers, including sells, withdrawals, or migration.
- The hook program may be upgradeable.
- The mint's transfer-hook authority may change the configured program.
- The validation `ExtraAccountMetaList` may change independently.
- Hooks can add compute, accounts, writable contention, setup costs, or user-specific failure modes.
- Some hook rules may require routing or setup that generic aggregators do not support.

Resolve accounts against sufficiently fresh mint and validation-list state. Treat account data as untrusted and validate ownership, lengths, and expected formats before decoding it.

The local SDK interface re-fetches each mint and validation list for every transfer and rejects mismatched owners, keys, mint association, truncated layouts, and invalid Execute-list markers. Its source trait receives typed state and delegates raw TLV parsing/PDA resolution; that boundary is not a substitute for production RPC response validation.

Authority policy meanings in the model:

- **PlatformRetained:** the configured platform authority can update rules; changing mint, hook program, or authority policy is rejected.
- **ImmutableAtLaunch:** engine reconfiguration is rejected after initialization.
- **GovernedTimelock:** the model accepts an authority token representing timelock approval. It does not verify signatures, delay, governance execution, or on-chain account constraints.

These are model semantics, not deployed access controls. A production hook must enforce authority and timelock proofs on-chain.
