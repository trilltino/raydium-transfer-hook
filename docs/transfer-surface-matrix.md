# Raydium instructions

Two instructions are added to Raydium so each transfer of a swap can carry its own hook accounts.
Everything else is unchanged. The code lives in the external forks pinned in `upstream.lock.toml`
(see [source-lock.md](source-lock.md)), not in this repository.

## Policy

* Existing Raydium discriminators and fixed account layouts stay **frozen** for non-hook users.
* Extend an instruction only when its parser defines an unambiguous per-transfer account boundary
  and its CPI helper forwards that exact slice.
* Use a new discriminator when the current remaining-account contract cannot safely express the hook
  tail.

Anchor discriminators are the first eight bytes of `SHA256("global:<snake_case_name>")`.

## The two new instructions

### CPMM `swap_base_input_v2`

Discriminator `[179, 135, 209, 217, 135, 75, 40, 58]`. Same 13 fixed accounts as V1
(`swap_base_input`, `[143, 190, 90, 218, 196, 30, 51, 222]`). Arguments:
`amount_in: u64, minimum_amount_out: u64, input_hook_account_count: u16,
output_hook_account_count: u16`.

Remaining accounts are exactly the **input transfer's slice, then the output transfer's slice**. Each
count is the full slice length: the resolved extra accounts plus the hook program and the validation
list (N + 2). Zero means no hook for that leg. Trailing accounts, or a nonzero slice shorter than two
accounts, are rejected. The user signs the input transfer; the pool authority PDA signs the output.

### CLMM `swap_v3`

Discriminator `[240, 224, 38, 33, 176, 31, 241, 175]`. Same 13 fixed accounts and legacy arguments as
`swap_v2` (`[43, 4, 237, 11, 26, 201, 30, 98]`), then `tick_array_count`, `bitmap_count`,
`input_hook_account_count`, `output_hook_account_count`, each `u16`.

Remaining accounts are exactly **tick arrays, bitmap accounts, the input slice, then the output
slice**. The bitmap count is at most one; transfer counts are zero or at least two. Counts are
checked against the remaining accounts and their data, and no trailing accounts are accepted. The
user signs the input; the pool-state PDA signs the output.

### Why a new instruction

CPMM V1 does not consume remaining accounts at all, and CLMM `swap_v2` already scans them as tick
arrays and a bitmap, so it cannot safely receive an unframed hook tail. The new instructions carry
explicit counts so the program knows where each slice starts and ends. The counts are framing only:
Token-2022 still validates the hook program, validation list and resolved accounts during the CPI.
The SDK never merges or deduplicates accounts across the two legs.

## Surface support

Status ladder: `unsupported` < `designed` < `implemented (external branch)` < `in-process verified` <
`local validator verified` < `devnet verified` < `official Raydium deployed`. "Devnet verified" means our fork builds under our
own program ids ([devnet.md](devnet.md)). No row is "official Raydium deployed".

| Surface | Status | Evidence |
|---|---|---|
| CPMM `swap_base_input_v2` | **devnet verified** | Real CPMM program: `cpmm_swap_base_input_v2_runtime.rs` and `local_flows.rs` (in-process, seven different hooks), `cargo xtask localnet e2e` (a real `solana-test-validator`, every hook and the starter built from source, from a clean checkout; CI), and the devnet transactions listed in [devnet.md](devnet.md). Both directions, with refusals. |
| CLMM `swap_v3` | **devnet verified** | Real CLMM pool, tick arrays and position: `clmm_swap_v3_runtime.rs` and `local_flows.rs` (in-process, seven different hooks), `cargo xtask localnet e2e` (as for CPMM), and the devnet transactions listed in [devnet.md](devnet.md). Both directions, with refusals. |
| CPMM `swap_base_input` (V1), CLMM `swap_v2` (V1) | unchanged; hooked mints rejected | host unit tests; V1 byte-identity pinned by SDK golden fixtures |
| CPMM `swap_base_output_v2` (exact output) | **devnet verified** | Real CPMM program: `local_flows.rs` (`cpmm_exact_output_*`: reference hook, the same hook on both legs, a transfer-fee mint; in-process against the SBF artifacts) and the `e2e --exact-output` run on the devnet CPMM upgraded to this instruction ([devnet.md](devnet.md)). Both directions, received amount exact, a too-low limit refused with nothing moved. Hooks that cap swaps per slot are not in the in-process exact-output set (the slot does not advance there); the hook's own refusal was seen coming through `swap_base_output_v2` as its error code. The exact-output mode of CLMM `swap_v3` was not run. |
| CPMM `initialize_v2`, `deposit_v2`, `withdraw_v2`, `collect_protocol_fee_v2`, `collect_fund_fee_v2` | **devnet verified** | Real CPMM program with the hook live: `local_flows.rs` (`cpmm_liquidity_*`: pool created with the hook already on, deposit, withdraw, both fee collections, and an over-limit deposit refused by the hook with nothing moved). The same flow ran on the devnet CPMM, upgraded to these instructions in place (`e2e --liquidity --exact-output`; transactions in [devnet.md](devnet.md)). |
| CPMM `initialize_with_permission_v2`, `collect_creator_fee_v2`, `collect_creator_fee_permissionless_v2` | **devnet verified** | Real CPMM program: `local_flows.rs` (`cpmm_liquidity_*`) and the `e2e --liquidity` run on the devnet CPMM. The flow has the admin create a permission record and a third AmmConfig with a creator fee, creates a permissioned pool with the hook live on both seed transfers, swaps both ways so creator fees accrue in both tokens, then collects them with `collect_creator_fee_v2` and, after more swaps, `collect_creator_fee_permissionless_v2`; both transfers of each collection run the hook and the creator's balances rise. |
| CPMM `swap_base_output` (V1), `deposit`, `withdraw`, fee collection, `initialize*` (the original instructions) | unchanged; hooked mints rejected | host unit tests; the `_v2` instructions above are the hook-aware ones |
| CLMM `open_position_v3`, `open_position_with_token22_nft_v3`, `increase_liquidity_v3`, `decrease_liquidity_v3`, `collect_protocol_fee_v2`, `collect_fund_fee_v2` | **devnet verified** | Real CLMM program with the hook live: `local_flows.rs` (`clmm_liquidity_*`: reference hook, the same hook on both legs, a transfer-fee mint; in-process against the SBF artifacts), `cargo xtask localnet e2e` and the `e2e --amm clmm --liquidity` run on the devnet CLMM, upgraded to these instructions in place ([devnet.md](devnet.md)). The flow opens a position with the hook running on both deposits, adds liquidity, swaps both ways, collects the position's fees (`decrease_liquidity_v3` with zero liquidity), collects protocol and fund fees, removes liquidity, and has an over-limit deposit refused by the hook with nothing moved. The hook slices are the last remaining accounts, token 0's then token 1's, followed by two `u16` counts, as for the swaps. |
| CLMM limit orders: `open_limit_order_v2`, `increase_limit_order_v2`, `decrease_limit_order_v2`, `settle_limit_order_v2` | **devnet verified** | Real CLMM program with the hook live: `local_flows.rs` (`clmm_liquidity_*`). One order sells the hooked token and one buys it; the deposit, the top-up, the refund and the payout of filled output each run the hook, swaps move the price across both orders' ticks to fill them, and an over-limit order is refused with nothing moved. An instruction's hook slices are its order's input token's, then its output token's, last in the remaining accounts, with two `u16` counts; a token the instruction does not move must have a count of zero. `close_limit_order` moves no tokens. The same flow ran on a local validator and on the devnet CLMM, upgraded to these instructions in place (`e2e --amm clmm --liquidity`; transactions in [devnet.md](devnet.md)). |
| CLMM reward emissions with a hooked reward mint: `initialize_reward_v2`, `set_reward_params_v2`, `collect_remaining_rewards_v2`, `decrease_liquidity_v4` (pays rewards) | **local verified** in-process; the funding is **devnet verified** and also ran on a local validator | A reward period lasts at least seven days, so everything but the funding needs the clock to move, which only the in-process bank does (`local_flows.rs`: `clmm_reward_emissions_in_a_hooked_token`). There the hooked token is also the reward token: funding, two days of accrual paid to a position, a seven-day extension with a top-up, and the unemitted part returned to the funder each run the hook. `decrease_liquidity_v4` takes the slices of token 0, token 1 and rewards 0 to 2, in that order, with five `u16` counts, and while any reward is initialised every decrease must pass its reward group. The funding was also run on the devnet CLMM (`e2e --amm clmm --liquidity --rewards`; the rest is recorded there as not run). The reward mint needs the pool admin's per-mint approval like any hooked mint. |
| LaunchLab | **blocked** | The on-chain handler is closed source, so nothing can be patched or tested |

**Pool creation with a hooked mint is gated upstream.** CPMM `initialize` and CLMM `create_pool` admit a
Token-2022 mint only if its extensions are on a short list (`TransferHook` is not) or the pool admin
(or a delegated owner) has created a `SupportMintAssociated` record for it. This is Raydium's existing
mint admission, unchanged by the forks, and it is per mint, not per hook program; see
[Raydium's mint admission](#raydiums-mint-admission-a-real-gate-and-what-it-is).

Every transfer helper that has no hook framing passes an empty slice and rejects a hook-enabled
mint, so an unsupported path fails with a clear error instead of an opaque Token-2022 one. That is
why liquidity deposits and withdrawals do not accept hooked mints yet: the hook goes on **after** a
pool has liquidity.

## Where to start extending to another surface

These are the handler files with token-transfer calls in the pinned sources. Each surface still
needs its own account, authority and reachability review before a patch.

| Program | Files with transfer calls |
|---|---|
| CPMM | `instructions/{initialize, initialize_with_permission, swap_base_input, swap_base_output, deposit, withdraw, collect_creator_fee, collect_creator_fee_permissionless}.rs`, `admin/{collect_fund_fee, collect_protocol_fee}.rs`, helpers in `utils/token.rs` |
| CLMM | `instructions/{swap, swap_v2, open_position, decrease_liquidity, collect_remaining_rewards, initialize_reward, set_reward_params}.rs`, `admin/{collect_fund_fee, collect_protocol_fee}.rs`, `limit_order/{decrease, increase, open, settle}_limit_order.rs`, helpers in `util/token.rs` |

## LaunchLab

The public SDK exposes LaunchLab's instruction builders and discriminators but not its on-chain
handlers, and the owner confirmed the handler is closed source. Its transfer count, signer seeds,
remaining-account parsing and migration behavior are therefore **unknown**, not inferred from
instruction names, and a production integration is blocked. The SDK repository is GPL-3.0 and is
never copied into this Apache-2.0 project. Do not change or claim a LaunchLab layout without an
authorized, executable integration surface.

## Raydium's mint admission (a real gate, and what it is)

Raydium's CPMM and CLMM already restrict which Token-2022 mints can start a pool. In the pinned
upstream CPMM (`utils/token.rs`, `is_supported_mint`) a Token-2022 mint is admitted only if every
extension it carries is on a short list (transfer fee, metadata pointer, token metadata,
interest-bearing, scaled UI amount), **or** a `SupportMintAssociated` record exists for it.
`TransferHook` is not on the list. The record can only be created by the pool admin or a delegated
"create support mint" owner. So:

* **Creating a pool with a hooked mint needs a human approval from whoever runs that Raydium
  deployment**, per mint. This is upstream behaviour that predates this work; the forks only add
  `swap_base_input_v2` and `swap_v3`.
* **The gate is about the mint, not the hook program.** The same record admits a mint pointing at
  any hook (or none yet: the flows approve the mint while its hook is unset and attach the hook
  after the pool has liquidity). Nothing names or ranks a hook program.
* **Once a pool exists,** swaps through it do not consult the record again.

What this means for the claim: *hook programs* are permissionless; *getting a hooked mint into a
Raydium pool* is not, and depends on the operator. On our integration builds the operator is the
deployer key, so every flow here runs `create_support_mint` as admin first. A launchpad that wants
hooked launches without per-mint approval would need that delegated-owner role, or Raydium to
change the rule.
