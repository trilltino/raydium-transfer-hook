# Transfer-surface matrix

## Source review

| Project | Reviewed revision | Source |
|---|---|---|
| Raydium CPMM | `b3187ae53a1b95a201f855a59024a12ca8f5b51a` | [`swap_base_input.rs`](https://github.com/raydium-io/raydium-cp-swap/blob/b3187ae53a1b95a201f855a59024a12ca8f5b51a/programs/cp-swap/src/instructions/swap_base_input.rs), [`deposit.rs`](https://github.com/raydium-io/raydium-cp-swap/blob/b3187ae53a1b95a201f855a59024a12ca8f5b51a/programs/cp-swap/src/instructions/deposit.rs), [`withdraw.rs`](https://github.com/raydium-io/raydium-cp-swap/blob/b3187ae53a1b95a201f855a59024a12ca8f5b51a/programs/cp-swap/src/instructions/withdraw.rs), [`utils/token.rs`](https://github.com/raydium-io/raydium-cp-swap/blob/b3187ae53a1b95a201f855a59024a12ca8f5b51a/programs/cp-swap/src/utils/token.rs) |
| Raydium CLMM | `ed1eb41519d5355755f7df52b43fa9610938b60b` | [`swap_v2.rs`](https://github.com/raydium-io/raydium-clmm/blob/ed1eb41519d5355755f7df52b43fa9610938b60b/programs/amm/src/instructions/swap_v2.rs) |
| Raydium LaunchLab client | `cc33ec28a8921a35609e83293e9e07ad830b0779` | [`instrument.ts`](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/launchpad/instrument.ts), [`layout.ts`](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/launchpad/layout.ts) |
| SPL Transfer Hook interface | `ec7063291e968f4b0064e4df0324ff49dcf320df` | [`offchain.rs`](https://github.com/solana-program/transfer-hook/blob/ec7063291e968f4b0064e4df0324ff49dcf320df/interface/src/offchain.rs), [`lib.rs`](https://github.com/solana-program/transfer-hook/blob/ec7063291e968f4b0064e4df0324ff49dcf320df/interface/src/lib.rs) |
| Token-2022 program | `b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e` | [`processor.rs`](https://github.com/solana-program/token-2022/blob/b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e/program/src/processor.rs), [`onchain.rs`](https://github.com/solana-program/token-2022/blob/b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e/program/src/onchain.rs), [`transfer_hook/processor.rs`](https://github.com/solana-program/token-2022/blob/b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e/program/src/extension/transfer_hook/processor.rs) |
| SPL Transfer Hook CPI helper | `ec7063291e968f4b0064e4df0324ff49dcf320df` | [`onchain.rs`](https://github.com/solana-program/transfer-hook/blob/ec7063291e968f4b0064e4df0324ff49dcf320df/interface/src/onchain.rs), [`instruction.rs`](https://github.com/solana-program/transfer-hook/blob/ec7063291e968f4b0064e4df0324ff49dcf320df/interface/src/instruction.rs) |

Raydium source is not vendored here. CP-Swap and CLMM are external programs pinned in `../upstream.lock.toml`; the hook-aware entrypoints below live on external fork branches (see `source-lock.md` for the exact commits). "Handler" columns describe those external branches.

## Raydium instruction facts and decision

Anchor discriminator bytes below are the first eight bytes of `SHA256("global:<snake_case_name>")`. Fixed-account counts exclude all remaining accounts.

| Instruction | Discriminator bytes | Fixed accounts / mint availability | Transfer count and direction | Remaining accounts and authority | Hook decision |
|---|---|---|---|---|---|
| CPMM `swap_base_input(amount_in: u64, minimum_amount_out: u64)` | `[143, 190, 90, 218, 196, 30, 51, 222]` | 13 fixed; input/output mints are explicit | 2: user input account → input vault; output vault → user output account | No handler parser/forwarding for remaining accounts. User payer signs input; pool authority PDA signs output. | Keep V1 unchanged for no-hook. Hooked transfers cannot work until each hook slice is plumbed into each CPI. No union is adopted. |
| CPMM `deposit(lp_token_amount: u64, maximum_token_0_amount: u64, maximum_token_1_amount: u64)` | `[242, 35, 198, 137, 82, 225, 242, 182]` | 13 fixed; both vault mints are explicit | 2: owner token accounts → token-0/token-1 vaults | No handler parser/forwarding for remaining accounts. Owner signs both inputs. | ABI unchanged; shared helper rejects hook-enabled mints because this entrypoint has no per-leg framing. |
| CPMM `withdraw(lp_token_amount: u64, minimum_token_0_amount: u64, minimum_token_1_amount: u64)` | `[183, 18, 70, 156, 148, 109, 161, 34]` | 14 fixed; both vault mints are explicit | 2: token-0/token-1 vaults → owner token accounts | No handler parser/forwarding for remaining accounts. Pool authority PDA signs both outputs. | ABI unchanged; shared helper rejects hook-enabled mints because this entrypoint has no per-leg framing. |
| CPMM `swap_base_input_v2(amount_in: u64, minimum_amount_out: u64, input_hook_account_count: u16, output_hook_account_count: u16)` | `[179, 135, 209, 217, 135, 75, 40, 58]` | Same 13 fixed accounts as V1; both mints are explicit | 2: user input → input vault; output vault → user output | Remaining accounts are exactly input hook slice then output hook slice. User signs input; pool authority PDA signs output. | Implemented on the external hook-support branch (host unit tests only; no validator execution). Parses exact count framing and forwards each leg to its Token-2022 CPI. |
| CLMM `swap_v2(amount: u64, other_amount_threshold: u64, sqrt_price_limit_x64: u128, is_base_input: bool)` | `[43, 4, 237, 11, 26, 201, 30, 98]` | 13 fixed; input/output vault mints are explicit | 2: user input → pool vault; pool vault → user output | Remaining accounts are scanned as tick arrays or bitmap extension. User signs input; pool-state PDA signs output. | Preserved unchanged. Its shared transfer helpers reject hook-enabled mints when no hook slice is supplied. |
| CLMM `swap_v3(amount, other_amount_threshold, sqrt_price_limit_x64, is_base_input, tick_array_count, bitmap_count, input_hook_account_count, output_hook_account_count)` | `[240, 224, 38, 33, 176, 31, 241, 175]` | Same 13 fixed accounts as SwapV2; both mints are explicit | 2: user input → pool vault; pool vault → user output | Remaining accounts are exactly tick arrays, bitmap extension (0 or 1), input hook slice, output hook slice. User signs input; pool-state PDA signs output. | Implemented on the external hook-support branch (host unit tests only; no validator execution). Validates exact section boundaries and forwards logical input/output hook slices in either swap direction. |

CPMM non-versioned transfer instructions still call the compatibility helpers with an empty hook slice, which explicitly rejects a mint with a Transfer Hook extension. CLMM's shared transfer helpers use the same fail-closed behavior; direct Token-2022 CPIs in limit-order open/increase/settle explicitly check and reject hooked mints because their remaining-account contracts do not define hook framing.

## Initial handler transfer-call inventory

This inventory comes from direct searches of the pinned handler sources. It is a starting audit, not approval to add hook behavior; each surface still needs account, authority, and reachability review before a patch.

| Program area | Handler source files with token-transfer calls | Current status |
|---|---|---|
| CP-Swap | `instructions/initialize.rs`, `initialize_with_permission.rs`, `swap_base_input.rs`, `swap_base_output.rs`, `deposit.rs`, `withdraw.rs`, `collect_creator_fee.rs`, `collect_creator_fee_permissionless.rs`, `admin/collect_fund_fee.rs`, `admin/collect_protocol_fee.rs`; shared helpers are `utils/token.rs` | `swap_base_input_v2` forwards framed hook slices. Other helper-based transfer paths use an empty slice and reject hook-enabled mints. |
| CLMM | `instructions/swap.rs`, `swap_v2.rs`, `open_position.rs`, `decrease_liquidity.rs`, `collect_remaining_rewards.rs`, `initialize_reward.rs`, `set_reward_params.rs`, `admin/collect_fund_fee.rs`, `admin/collect_protocol_fee.rs`, `limit_order/decrease_limit_order.rs`, `limit_order/increase_limit_order.rs`, `limit_order/open_limit_order.rs`, `limit_order/settle_limit_order.rs`; shared helpers are `util/token.rs` | `swap_v3` forwards framed swap slices. Other shared-helper paths fail closed on hooked mints; direct limit-order open/increase/settle paths explicitly reject hooked mints. SwapV2 remains unchanged and cannot accept an unframed tail. |
| LaunchLab | No matching on-chain handler source available in this workspace; the public SDK reference is `raydium-io/raydium-sdk-V2` | User confirmed the handler is closed source. Its CPI count, signer seeds, transfer surfaces, and graduation behavior cannot be source-verified here. Do not claim the LaunchLab lifecycle E2E or infer handler behavior from SDK builders. |

## LaunchLab source limits

The reviewed public LaunchLab SDK exposes exact instruction builders and discriminators but not the on-chain Rust handlers. The owner confirmed the handler is closed source. The SDK's `initializeWithToken2022` discriminator is `[37, 190, 126, 222, 44, 154, 171, 17]`; it marks `mintA` as a writable signer and includes Token-2022 as the mint program. Its extension parameters include transfer-fee configuration, not a Transfer Hook extension or validation-list setup. The public SDK repository is GPL-3.0 and is not copied into this Apache-2.0 project.

`buyExactIn` is `[250, 234, 13, 123, 213, 156, 19, 236]`; `sellExactIn` is `[149, 39, 222, 155, 211, 124, 152, 26]`. Each SDK builder carries both mint keys and both token program keys, a 15-account base list, optional share-fee receiver, and three trailing system/fee-vault accounts. The SDK source does not establish handler-side transfer count, CPI authority seeds, remaining-account parsing, or migration transfer behavior. These facts are therefore recorded as **unknown**, not inferred from instruction names.

The local LaunchLab crate models a selected hook setup and same-mint graduation persistence only; it is not a patch or ABI assertion about the deployed LaunchLab program.

## SPL account-list contract

At the reviewed SPL interface revision, the validation-list PDA is derived from `["extra-account-metas", mint]` under the hook program. `ExecuteInstruction` uses discriminator bytes `[105, 37, 101, 197, 75, 251, 102, 26]` followed by the transfer amount as little-endian `u64`. Its account prefix is source, mint, destination, authority; the validation list follows, then the ordered accounts decoded/resolved from that list.

The Token-2022 transfer processor reads the mint's current hook program, marks source/destination hook-transfer state, invokes the hook, then clears those flags. Its on-chain helper finds the hook executable and validation-list account among caller-supplied additional accounts and resolves the extra-meta list for the Execute CPI. Consequently, a caller must forward the hook executable, validation-list PDA, and all exact extra accounts to the Token-2022 transfer CPI. A hook error propagates as a failed transfer and Solana transaction atomicity rolls back the enclosing instruction.

The SPL off-chain helper appends the resolved extra metas followed by the hook program id and validation-list account to the caller's instruction. Resolution is for each concrete transfer context. The SDK delegates TLV decoding, PDA derivation and extra-account resolution to the official `spl-transfer-hook-interface` helpers for each concrete transfer, then frames the resulting slices for Raydium.

## Repository change decision

Existing V1 discriminators and non-hook paths are unchanged. Hook-aware behavior is added only as
new versioned instructions where the ABI needs per-transfer framing (CPMM `swap_base_input_v2`, CLMM
`swap_v3`); it is implemented in the external fork branches, not in this repository. Helper-based
paths with no hook framing reject hooked mints early. LaunchLab cannot be patched or runtime-tested
from public source, so a production LaunchLab integration is blocked.

## Support status per surface

Status ladder, in order: `unsupported` < `designed` < `implemented (external branch)` <
`localnet verified` < `integration-devnet verified` < `official Raydium deployed`. A surface is only
as supported as its highest *evidenced* rung.

Base/hook revisions: see [`source-lock.md`](./source-lock.md). Official-deployment status is
`not deployed` for every row; no upstream PR exists.

| Surface | Hook-aware discriminator | Status | Evidence |
|---|---|---|---|
| CPMM `swap_base_input_v2` | `[179, 135, 209, 217, 135, 75, 40, 58]` | **integration-devnet verified** | Real CPMM program: `cpmm_swap_base_input_v2_runtime.rs` and `local_flows.rs` (in-process); devnet transactions in [integration-devnet](./integration-devnet.md). Both directions, a refusal in each, two different hooks. |
| CPMM `swap_base_input` (V1) | n/a (unchanged) | unchanged; hooked mints rejected | host unit tests; V1 byte-identity pinned by SDK golden fixtures |
| CPMM `swap_base_output`, `deposit`, `withdraw`, fee collection, `initialize*` | none | unsupported: helper rejects hooked mints | host unit tests |
| CLMM `swap_v3` | `[240, 224, 38, 33, 176, 31, 241, 175]` | **integration-devnet verified** | Real CLMM pool, tick arrays and position: `clmm_swap_v3_runtime.rs` and `local_flows.rs` (in-process); devnet transactions in [integration-devnet](./integration-devnet.md). Both directions, a refusal in each, two different hooks. |
| CLMM `swap_v2` (V1) | n/a (unchanged) | unchanged; hooked mints rejected | host unit tests; V1 byte-identity pinned by SDK golden fixtures |
| CLMM positions, liquidity, rewards, fees | none | unsupported: helper rejects hooked mints | host unit tests |
| CLMM limit orders (open/increase/settle) | none | unsupported: explicit rejection | host unit tests |
| LaunchLab | none | designed; blocked (handler source not public) | none |

"integration-devnet verified" means our fork builds under our own program ids on Solana devnet.
Official Raydium, including its devnet, does not contain these instructions (no upstream PR exists),
so no row is "official Raydium deployed". "In-process" is `solana-program-test`: the real runtime
executing the real binaries, not a validator process.

Fixed accounts, transfer counts, authorities and remaining-account grammars for the implemented
rows are in the instruction table above. The remaining unsupported rows have not had a per-surface
source audit recorded here yet (source, destination, signer seeds, remaining-account semantics);
the transfer-call inventory above is only a starting list.
