# Instruction versioning

## Policy

- Keep existing Raydium discriminators and fixed account layouts frozen for non-hook users.
- Extend an instruction only when its parser defines an unambiguous per-transfer account boundary and its CPI helper forwards that exact slice.
- Use a new discriminator when the current remaining-account contract cannot safely express the hook tail.

## Reviewed decision

The CPMM and CLMM source facts and the exact V1/SwapV2 discriminator bytes are recorded in the [transfer-surface matrix](transfer-surface-matrix.md). Existing V1/SwapV2 ABIs remain unchanged.

- CPMM V1 remaining accounts are not consumed by the reviewed handlers/helpers. `swap_base_input_v2` appends `input_hook_account_count: u16` and `output_hook_account_count: u16` after the legacy arguments and has discriminator `[179, 135, 209, 217, 135, 75, 40, 58]`. Remaining accounts are exactly the input transfer slice followed by the output transfer slice; each count is the full slice length, including resolved ExtraAccountMeta accounts plus the hook program and validation-list accounts (N+2). Zero means no hook accounts for that leg. Trailing accounts or a nonzero slice shorter than two accounts are rejected.
- CLMM SwapV2 consumes remaining accounts as tick-array/bitmap state and cannot safely receive an unframed hook tail. `swap_v3` has discriminator `[240, 224, 38, 33, 176, 31, 241, 175]`, keeps SwapSingleV2's fixed accounts and legacy arguments, then carries explicit `tick_array_count`, `bitmap_count`, `input_hook_account_count`, and `output_hook_account_count` as `u16` fields. Remaining accounts are exactly tick arrays, bitmap accounts, input-transfer slice, then output-transfer slice. Bitmap count is at most one; transfer counts are zero or at least two. Counts are checked against remaining accounts and required account data, and no trailing accounts are accepted.
- Both formats preserve the SDK resolver's ordered per-transfer slice and do not merge or deduplicate accounts across transfer legs. The V2/V3 counts are framing only; Token-2022 still validates the hook program, validation list, and ExtraAccountMeta resolution during CPI.
- Other CP-Swap and CLMM transfer-helper callsites pass an empty hook slice and reject hook-enabled mints. CLMM limit-order open/increase/settle paths that issue direct Token-2022 CPIs explicitly reject hook-enabled mints because their remaining-account contracts are not framed for per-transfer extras.
- The LaunchLab lifecycle and migration semantics are not source-verifiable: the user confirmed its handler is closed source. Do not change or claim the deployed LaunchLab instruction layout without an authorized executable integration surface.

These are the selected versioned formats for the external hook-support CPMM/CLMM branches (see `upstream.lock.toml`). Unit tests pin their discriminators, encoded count bytes, account ranges, malformed-frame errors, and V1/SwapV2 regression behavior. The local integration models and ranges are not an ABI definition.
