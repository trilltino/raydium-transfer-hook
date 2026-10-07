# Fair launch

Limits on **buys** during a launch window: how big a buy can be, how much one account can hold, how
many buys can land in one slot, and how high a priority fee a buy transaction may declare.

**The rule is in [`src/rule.rs`](src/rule.rs). It is pure: no accounts, no Solana types.**
Everything else in this folder is plumbing you can leave alone.

A **buy** is a transfer out of the pool's vault of the hooked token. Outside the window, and for
every transfer that is not a buy (sells, wallet-to-wallet moves), nothing is checked.

| Check | Stops | Error |
|---|---|---|
| buy is at most `max_buy` | one wallet vacuuming up the pool in a swap | `0xB003` |
| balance after the buy is at most `max_wallet` | one account accumulating a big share | `0xB004` |
| at most `max_buys_per_slot` buys in a slot | bundles: many buys packed into one block | `0xB005` |
| declared priority fee at most `max_priority_micro_lamports` | winning the block by outbidding everyone | `0xB006` |

## The decision

```rust
// src/rule.rs
pub fn check_buy(params: &Params, buy: &Buy) -> Result<(), FairLaunchError> {
    if buy.amount > params.max_buy { return Err(FairLaunchError::PerBuyCapExceeded); }
    if buy.wallet_balance_after > params.max_wallet { return Err(FairLaunchError::MaxWalletExceeded); }
    if buy.buys_in_slot > params.max_buys_per_slot { return Err(FairLaunchError::TooManyBuysInSlot); }
    if params.max_priority_micro_lamports > 0
        && buy.priority_micro_lamports.unwrap_or(0) > params.max_priority_micro_lamports
    { return Err(FairLaunchError::PriorityFeeTooHigh); }
    Ok(())
}
```

## What is in this folder

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Params`, `check_buy`, the slot counter and fee parsing. Unit-tested alone. |
| `src/config.rs` | The per-mint config (window, limits, pool vault) and the slot counter. |
| `src/instruction.rs` | The one setup instruction, `Initialize`. |
| `src/processor/` | `Initialize` and `Execute`, built on [`hook-kit`](../../crates/hook-kit). |
| `src/error.rs` | Error codes from `0xB001`. |
| `tests/fair_launch.rs` | Runtime tests inside real Token-2022 transfers. |

## Accounts

Three extra accounts per transfer, so a swap leg is 5 accounts: the config PDA (read-only), the
slot counter PDA (**writable**), the instructions sysvar (read-only), then the hook program and the
validation list. An integrator must name the counter as an allowed writable account
(`PrivilegePolicy::allowing_writable`), or the SDK refuses the leg.

## Run it

```bash
cargo test -p fair-launch-hook                       # native
cargo build-sbf --manifest-path templates/fair-launch/Cargo.toml --sbf-out-dir target/integration-sbf
SBF_OUT_DIR=target/integration-sbf cargo test -p fair-launch-hook   # the real SBF binary
```

Through Raydium: `cpmm_with_the_fair_launch_template` and `clmm_with_the_fair_launch_template` in
`tests/program-test/tests/local_flows.rs`. They check that ordinary swaps pass; that a buy
over the cap, three buys in one transaction, and a buy declaring a high priority fee are each
refused with the hook's own code and roll back; and that the oversized buy succeeds after the
window closes.

## Honest limits

* **`max_wallet` is per token account, not per person.** Someone can open many accounts. The slot
  budget is what slows that, because all their buys share it.
* **The slot budget is per mint, not per buyer.** It is a launch-wide budget: when it is used up,
  honest buyers in that slot are refused too. That is the price of stopping a bundle.
* **The fee check sees the fee a legacy or v0 transaction declares.** It reads
  `SetComputeUnitPrice` from the transaction's top-level instructions. It cannot see a tip paid to a
  block builder as a plain transfer, so it limits ordinary fee wars, not private bundles.
* **The fee check does not work on v1 transactions.** In the v1 format (SIMD-0385, live on mainnet
  since September 2026) the priority fee is a field of the message and `ComputeBudget` instructions
  are no-ops, so the check is bypassed (pay any fee, declare none) or fooled (declare a price the
  runtime ignores). Where v1 transactions are accepted, do not rely on this check; the buy-size,
  account and slot limits do not depend on the transaction format. See
  [Solana Compass on SIMD-0385](https://solanacompass.com/news/transaction-v1-simd-0385-is-live-on-solana-mainnet-at-epoch-1035)
  and the [Chainstack v1 guide](https://docs.chainstack.com/docs/solana-transaction-v1).
* **Contention.** The counter is a writable account in every transfer of the mint, buys or not, so
  transfers of this mint in the same block serialise on it. Fine for a launch window; a reason to
  keep the window short.
* **One pool vault.** A buy is a transfer out of the configured vault. A second pool needs another
  commitment (or a different hook).
* **The program's upgrade authority can replace this rule.** Disclose it or revoke it.
