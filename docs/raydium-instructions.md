# Raydium instructions

Two instructions are added to Raydium so each transfer of a swap can carry its own hook accounts.
Everything else is unchanged. The code lives in the external forks pinned in `upstream.lock.toml`
(see [upstream-sources.md](upstream-sources.md)), not in this repository.

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
`devnet verified` < `official Raydium deployed`. "Devnet verified" means our fork builds under our
own program ids ([devnet.md](devnet.md)). No row is "official Raydium deployed".

| Surface | Status | Evidence |
|---|---|---|
| CPMM `swap_base_input_v2` | **devnet verified** | Real CPMM program: `cpmm_swap_base_input_v2_runtime.rs` and `local_flows.rs` (in-process, five different hooks), and the devnet transactions listed in [devnet.md](devnet.md). Both directions, with refusals. |
| CLMM `swap_v3` | **devnet verified** | Real CLMM pool, tick arrays and position: `clmm_swap_v3_runtime.rs` and `local_flows.rs` (in-process, five different hooks), and the devnet transactions listed in [devnet.md](devnet.md). Both directions, with refusals. |
| CPMM `swap_base_input` (V1), CLMM `swap_v2` (V1) | unchanged; hooked mints rejected | host unit tests; V1 byte-identity pinned by SDK golden fixtures |
| CPMM `swap_base_output`, `deposit`, `withdraw`, fee collection, `initialize*` | unsupported: the shared helper rejects hooked mints with a clear error | host unit tests |
| CLMM positions, liquidity, rewards, fees | unsupported: the shared helper rejects hooked mints | host unit tests |
| CLMM limit orders (open, increase, settle) | unsupported: explicit rejection | host unit tests |
| LaunchLab | **blocked** | The on-chain handler is closed source, so nothing can be patched or tested; `integrations/launchlab` is a simulator, not an integration |

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
