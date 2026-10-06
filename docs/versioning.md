# Instruction versioning

## Policy

- Keep existing Raydium discriminators and fixed account layouts frozen for non-hook users.
- Extend an instruction only when its parser defines an unambiguous per-transfer account boundary and its CPI helper forwards that exact slice.
- Use a new discriminator when the current remaining-account contract cannot safely express the hook tail.

## Reviewed decision

The CPMM and CLMM source facts and the exact V1/SwapV2 discriminator bytes are recorded in the [transfer-surface matrix](transfer-surface-matrix.md). No on-chain ABI was changed in this workspace.

- CPMM V1 remaining accounts are not consumed by the reviewed handlers/helpers; forwarding separate hook slices will require handler/helper code in a Raydium fork. Keep current V1 behavior unchanged.
- CLMM SwapV2 consumes remaining accounts as tick-array/bitmap state and cannot safely receive an unframed hook tail. A future on-chain implementation should use a separately framed/new instruction rather than reinterpret existing remaining accounts.
- LaunchLab lifecycle and migration semantics are not source-verified; do not change its instruction layouts until the handler source is available.

The local integration models and ranges are not an ABI definition.
