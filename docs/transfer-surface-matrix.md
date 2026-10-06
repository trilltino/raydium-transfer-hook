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

## Raydium instruction facts and decision

Anchor discriminator bytes below are the first eight bytes of `SHA256("global:<snake_case_name>")`. Fixed-account counts exclude all remaining accounts.

| Instruction | Discriminator bytes | Fixed accounts / mint availability | Transfer count and direction | Remaining accounts and authority | Hook decision |
|---|---|---|---|---|---|
| CPMM `swap_base_input(amount_in: u64, minimum_amount_out: u64)` | `[143, 190, 90, 218, 196, 30, 51, 222]` | 13 fixed; input/output mints are explicit | 2: user input account → input vault; output vault → user output account | No handler parser/forwarding for remaining accounts. User payer signs input; pool authority PDA signs output. | Keep V1 unchanged for no-hook. Hooked transfers cannot work until each hook slice is plumbed into each CPI. No union is adopted. |
| CPMM `deposit(lp_token_amount: u64, maximum_token_0_amount: u64, maximum_token_1_amount: u64)` | `[242, 35, 198, 137, 82, 225, 242, 182]` | 13 fixed; both vault mints are explicit | 2: owner token accounts → token-0/token-1 vaults | No handler parser/forwarding for remaining accounts. Owner signs both inputs. | Same V1/no-hook decision; live hook path requires explicit per-leg plumbing. |
| CPMM `withdraw(lp_token_amount: u64, minimum_token_0_amount: u64, minimum_token_1_amount: u64)` | `[183, 18, 70, 156, 148, 109, 161, 34]` | 14 fixed; both vault mints are explicit | 2: token-0/token-1 vaults → owner token accounts | No handler parser/forwarding for remaining accounts. Pool authority PDA signs both outputs. | Same V1/no-hook decision; live hook path requires explicit per-leg plumbing. |
| CLMM `swap_v2(amount: u64, other_amount_threshold: u64, sqrt_price_limit_x64: u128, is_base_input: bool)` | `[43, 4, 237, 11, 26, 201, 30, 98]` | 13 fixed; input/output vault mints are explicit | 2: user input → pool vault; pool vault → user output | Remaining accounts are scanned by data length as tick arrays or bitmap extension, stopping at the first other account. User signs input; pool-state PDA signs output. | A hook tail is ambiguous under this parser and is not forwarded to transfer CPIs. Preserve this ABI; a future hooked entrypoint needs explicit framing/new discriminator. |

The CPMM helpers call Token-2022 `transfer_checked` with the fixed transfer accounts and do not call `with_remaining_accounts`. In CLMM `exact_internal_v2`, both transfer helpers also receive only fixed accounts; `ctx.remaining_accounts` is consumed for tick/bitmap state.

## LaunchLab source limits

The reviewed public LaunchLab SDK exposes exact instruction builders and discriminators but not the on-chain Rust handlers. The SDK's `initializeWithToken2022` discriminator is `[37, 190, 126, 222, 44, 154, 171, 17]`; it marks `mintA` as a writable signer and includes Token-2022 as the mint program. Its extension parameters include transfer-fee configuration, not a Transfer Hook extension or validation-list setup.

`buyExactIn` is `[250, 234, 13, 123, 213, 156, 19, 236]`; `sellExactIn` is `[149, 39, 222, 155, 211, 124, 152, 26]`. Each SDK builder carries both mint keys and both token program keys, a 15-account base list, optional share-fee receiver, and three trailing system/fee-vault accounts. The SDK source does not establish handler-side transfer count, CPI authority seeds, remaining-account parsing, or migration transfer behavior. These facts are therefore recorded as **unknown**, not inferred from instruction names.

The local LaunchLab crate models a selected hook setup and same-mint graduation persistence only; it is not a patch or ABI assertion about the deployed LaunchLab program.

## SPL account-list contract

At the reviewed SPL interface revision, the validation-list PDA is derived from `["extra-account-metas", mint]` under the hook program. `ExecuteInstruction` uses discriminator bytes `[105, 37, 101, 197, 75, 251, 102, 26]` followed by the transfer amount as little-endian `u64`. Its account prefix is source, mint, destination, authority; the validation list follows, then the ordered accounts decoded/resolved from that list.

The Token-2022 transfer processor reads the mint's current hook program, marks source/destination hook-transfer state, invokes the hook, then clears those flags. Its on-chain helper finds the hook executable and validation-list account among caller-supplied additional accounts and resolves the extra-meta list for the Execute CPI. Consequently, a caller must forward the hook executable, validation-list PDA, and all exact extra accounts to the Token-2022 transfer CPI. A hook error propagates as a failed transfer and Solana transaction atomicity rolls back the enclosing instruction.

The SPL off-chain helper appends the resolved extra metas followed by the hook program id and validation-list account to the caller's instruction. Resolution is for each concrete transfer context. The local SDK models this ordering through a provider abstraction; it does not parse the raw TLV format or derive real Solana PDAs.

## Repository change decision

No live instruction or fixed account layout has been modified in this workspace. Its integration modules are local flow models. Keep existing V1 discriminators and non-hook paths unchanged in any future upstream fork; do not ship hooked traffic until the upstream handler has an explicit, tested way to frame and forward the per-transfer accounts.
