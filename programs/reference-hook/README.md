# Reference hook engine

This crate is a dependency-free behavioral model, not an Anchor/Solana program and not deployable.

It implements per-mint configuration checks for allowed/active modules, rejects incompatible allow/deny address-list combinations, applies a transfer limit/address policy, and models the three platform authority policies. The governed-timelock authorization is a model token, not cryptographic proof.

A deployable implementation still needs the versioned SPL Transfer Hook Execute interface, a real validation-list account initialized with the official TLV account-resolution library, on-chain signer/PDA constraints, and runtime tests.
