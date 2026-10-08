# Fair Launch: launch participation controls

Basic bundle and snipe resistance for a token launch: caps on how big a buy can be, how much one
token account can hold, how many buys can land in one slot, and how high a priority fee a buy may
declare, all inside a launch window.

**The rule is [`src/rule.rs`](src/rule.rs). It is pure: no accounts, no Solana types.** The rest of
the folder is plumbing.

## WHAT

Four independent limits on **buys**, each switched off by setting it to `0` (at least one must be on):

| Limit | Refuses | Error |
|---|---|---|
| `max_buy` | one buy larger than the cap | `0xB003` `PerBuyCapExceeded` |
| `max_wallet` | a buy that leaves the buyer's token account above the cap | `0xB004` `MaxWalletExceeded` |
| `max_buys_per_slot` | the (n+1)th buy in the same slot | `0xB005` `TooManyBuysInSlot` |
| `max_priority_micro_lamports` | a buy whose transaction declares a higher compute-unit price | `0xB006` `PriorityFeeTooHigh` |

One program, three common settings:

| Setting | Configure | Effect |
|---|---|---|
| Launch participation controls | all four limits and a window | the table above |
| Per-slot budget only | only `max_buys_per_slot`, window `0..i64::MAX` | a pack of many buys in one block is refused as a whole |
| Snipe resistance | `max_buy` + `max_wallet`, a short window | no single account takes the pool in the first minutes |

## WHY

Launches get taken over in the first blocks: one actor buys most of the supply in one bundle, or
outbids everyone on priority fees. These controls make that more expensive and more visible. They do
**not** make a launch "fair" in an absolute sense; see LIMITATIONS.

## TRIGGER

A **buy** is a transfer **out of** one of the launch's *venue* accounts: token accounts of the hooked
mint (up to four, for example an AMM pool's vault of the token) named at `Initialize`. A buy is
checked only inside the window `[window_start, window_end)`. Everything else (sells, wallet-to-wallet
transfers, any transfer after the window) is not checked and is never refused.

## EXAMPLE

Limits: `max_buy = 1,000`, `max_wallet = 5,000`, `max_buys_per_slot = 3`, `max_priority = 1,000`
micro-lamports. A buyer holds 4,500.

| Transfer | Result |
|---|---|
| venue -> buyer, 500 (balance after: 5,000) | allowed (every limit is inclusive) |
| venue -> buyer, 600 (balance after: 5,100) | refused `0xB004` |
| venue -> buyer, 1,001 | refused `0xB003` |
| a 4th buy in the same slot (any buyer) | refused `0xB005` |
| a buy in a transaction that sets a price of 5,000 micro-lamports | refused `0xB006` |
| buyer -> venue (a sell) | allowed |
| venue -> buyer, 1,001, after `window_end` | allowed |

## RULES

```rust
// src/rule.rs
pub fn check_buy(params: &Params, buy: &Buy) -> Result<(), FairLaunchError> {
    if params.max_buy > 0 && buy.amount > params.max_buy { return Err(FairLaunchError::PerBuyCapExceeded); }
    if params.max_wallet > 0 && buy.wallet_balance_after > params.max_wallet { return Err(FairLaunchError::MaxWalletExceeded); }
    if params.max_buys_per_slot > 0 && buy.buys_in_slot > params.max_buys_per_slot { return Err(FairLaunchError::TooManyBuysInSlot); }
    if params.max_priority_micro_lamports > 0
        && buy.priority_micro_lamports.unwrap_or(0) > params.max_priority_micro_lamports
    { return Err(FairLaunchError::PriorityFeeTooHigh); }
    Ok(())
}
```

* The window is half-open: the first second counts, `window_end` does not.
* The slot counter restarts when the slot changes. All venues share one per-slot budget.
* The priority fee is read from `SetComputeUnitPrice` in the transaction's top-level instructions
  (through the instructions sysvar). No declared price counts as `0`.
* `Initialize` takes the venues as the accounts after the fixed ones: each must be a token account of
  the hooked mint, with no repeats.

## LIMITATIONS

**This is basic bundle/snipe resistance, not complete bundle detection.**

* **Per-wallet and per-token-account rules do not establish person-level identity.** `max_wallet` is
  per token account. A participant can split activity across many wallets or accounts and each stays
  under the cap.
* **Per-slot controls resist concentrated activity; they do not detect bundles.** Buying across
  several slots, from several accounts, or through a private bundle avoids the per-slot budget. The
  budget is also per mint, not per buyer: when it is used up, honest buyers in that slot are refused
  too. That is the price of slowing a bundle.
* **The fee check sees only the fee a legacy or v0 transaction declares.** It cannot see a tip paid
  to a block builder as a plain transfer, so it limits ordinary fee wars, not private bundles.
* **The fee check does not work on v1 transactions** (SIMD-0385). There the priority fee is a field
  of the message and `ComputeBudget` instructions are no-ops, so the check is bypassed (pay any fee,
  declare none) or fooled (declare a price the runtime ignores). Where v1 transactions are accepted,
  do not rely on this check; the size, account and slot limits do not depend on the format.
* **At most four venues.** A buy is a transfer out of a configured account. A venue you did not name
  is not a buy. More venues need another hook or another mint.
* **The per-slot setting's window never ends** unless you set a real `window_end`.
* After the window, nothing is limited.

## TRUST

* **Config authority:** `Initialize` is signed by the mint's live Transfer Hook authority. The
  config (window, limits, venues) **cannot be changed afterwards**: there is no update instruction,
  and a second `Initialize` is refused (`0x8006`).
* **Program upgrade authority:** whoever holds it can replace this rule for every mint that uses the
  program. Disclose it or revoke it (`solana program set-upgrade-authority <ID> --final`).
* **Mint hook selection:** the mint's Transfer Hook authority can re-point the mint at a different
  hook program, or the extension's authority can be revoked to make that permanent. Say which you did.

## STATE / COST

| Account | Access | Notes |
|---|---|---|
| config PDA `["config", mint]` | read-only | window, limits, venues |
| slot counter PDA `["counter", mint]` | **writable** | counts buys in the current slot; written only by buys but present in every transfer |
| instructions sysvar | read-only | only when `max_priority_micro_lamports` is set |
| hook program, validation list | read-only | appended by the caller as for any hook |

Two to three extras per transfer, so four or five hook accounts in total. **Contention:** the counter
is writable in every transfer of the mint, buys or not, so transfers of this mint serialise on it
within a block. That is acceptable for a launch window and a reason to keep the window short. Setup
rent is about 0.005 SOL per mint.

## TESTS

`cargo test` (from this directory). Rule unit tests are in `src/rule.rs` and `src/config.rs`; runtime
tests are in `tests/fair_launch.rs` and run inside real Token-2022 transfers.

| Behaviour | Test |
|---|---|
| valid / exact boundary of every limit | `a_buy_exactly_at_every_limit_passes`, `a_buy_at_the_cap_passes_and_one_token_over_is_refused` |
| rejection per limit, exact code | `one_over_each_limit_is_refused_with_its_own_error`, `a_wallet_cannot_accumulate_past_the_wallet_cap_across_buys`, `a_bundle_of_buys_in_one_slot_is_refused_as_a_whole`, `a_high_priority_fee_is_refused_and_a_declared_low_one_passes` |
| window edges | `the_window_is_half_open`, `nothing_is_checked_outside_the_window` |
| irrelevant transfer path | `selling_and_moving_tokens_between_wallets_are_never_limited` |
| state update | `the_slot_counter_counts_within_a_slot_and_restarts_in_the_next` |
| malformed config | `a_malformed_config_is_refused`, `validation_rejects_empty_windows_and_a_launch_with_no_limit` |
| unauthorized setup, re-init | `initialize_records_the_launch_and_validates_its_inputs` |
| per-slot only / several venues | `the_per_slot_budget_alone_slows_a_bundle`, `every_named_venue_draws_on_the_same_budget` |
| direct `Execute` call | `a_direct_execute_call_is_refused` |
| unauthorized *config update* | not applicable: the config cannot be updated |

```sh
cargo test                                     # in this directory
cargo build-sbf --sbf-out-dir target/deploy && SBF_OUT_DIR=$PWD/target/deploy cargo test   # real SBF binary
```

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Params`, `check_buy`, slot counter and fee parsing |
| `src/config.rs` | per-mint config (window, limits, venues) and the slot counter |
| `src/instruction.rs`, `src/processor/` | the one setup instruction and `Execute`, on [`hook-kit`](../../hook-kit) |
| `src/error.rs` | error codes from `0xB001` |
