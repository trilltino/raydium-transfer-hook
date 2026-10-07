# Anti-bundle

A per-slot budget on **buys** from recognised venues, so a bundle of many buys packed into one block
is refused as a whole.

**The rule is in [`src/rule.rs`](src/rule.rs). It is pure: no accounts, no Solana types.**
Everything else in this folder is plumbing you can leave alone.

```text
slot 100:  buy buy buy buy        <- with max_buys_per_slot = 3 the 4th is refused
slot 101:  buy buy                <- the budget starts over
```

A **buy** is a transfer out of one of the configured venue vaults (the pool's vault of this token).
Sells, wallet-to-wallet moves and everything else are never counted and never refused.

## The decision

```rust
// src/rule.rs
pub fn is_buy<K: PartialEq>(source: &K, venues: &[K]) -> bool { venues.contains(source) }

pub fn check_buy(params: &Params, buys_in_slot: u16) -> Result<(), AntiBundleError> {
    if buys_in_slot > params.max_buys_per_slot {
        return Err(AntiBundleError::TooManyBuysInSlot);
    }
    Ok(())
}
```

`processor/execute.rs` counts the buy into the slot's counter and calls the rule. A refused buy
fails its transaction, so the count it would have added rolls back with everything else.

## What is in this folder

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Params`, `is_buy`, the slot counter and `check_buy`. Unit-tested alone. |
| `src/state.rs` | The per-mint config (budget, up to 4 venues) and the slot counter. |
| `src/instruction.rs` | The one setup instruction, `Initialize`. |
| `src/processor/` | `Initialize` and `Execute`, built on [`hook-kit`](../../crates/hook-kit). |
| `src/error.rs` | Error codes from `0xD001`. |
| `tests/anti_bundle.rs` | Runtime tests inside real Token-2022 transfers. |

## Accounts

Two extra accounts per transfer, so a swap leg is 4 accounts: the config PDA (read-only), the slot
counter PDA (**writable**), then the hook program and the validation list. An integrator must name
the counter as an allowed writable account (`PrivilegePolicy::allowing_writable`).

## Errors

| Code | Name | When |
|---|---|---|
| `0xD001` | `InvalidParams` | a zero budget |
| `0xD002` | `InvalidVenues` | no venues, more than 4, or a venue listed twice |
| `0xD003` | `VenueMismatch` | a venue is not a token account of the hooked mint |
| `0xD004` | `TooManyBuysInSlot` | more buys in the slot than the budget |
| `0xD005` | `InvalidConfig` | wrong or malformed config |
| `0xD006` | `InvalidInstruction` | malformed setup instruction |
| `0xD007` | `InvalidState` | wrong, malformed or read-only counter |
| `0x8001..` | `hook_kit::KitError` | shared checks: wrong authority, direct `Execute`, already initialised, ... |

## Run it

```bash
cargo test -p anti-bundle-hook                       # native
cargo build-sbf --manifest-path templates/anti-bundle/Cargo.toml --sbf-out-dir target/integration-sbf
SBF_OUT_DIR=target/integration-sbf cargo test -p anti-bundle-hook   # the real SBF binary
```

Through Raydium: `cpmm_with_the_anti_bundle_template` and `clmm_with_the_anti_bundle_template` in
`tests/program-test/tests/local_flows.rs`. They check that ordinary swaps pass and that a
transaction with one swap too many in a slot is refused with `0xD004` and rolls back.

## Honest limits

* **The budget is per mint and slot, shared by everyone.** When it is spent, an honest buyer in the
  same slot is refused too. That is the price of stopping a bundle.
* **It does not limit one buyer across slots,** or one buyer using many accounts across slots.
  [`fair-launch`](../fair-launch) adds a per-account cap and a buy-size cap; use it, or combine the
  rules in one hook.
* **It only knows the venues it was configured with.** A second pool, or a route through a venue
  that was not configured, is not counted.
* **It counts transfers, not swaps.** A swap that buys from one venue counts once.
* **Contention.** The counter is a writable account in every transfer of the mint, buys or not, so
  transfers of this mint in the same block serialise on it.
* **The program's upgrade authority can replace this rule.** Disclose it or revoke it.
