//! # The rule: a fair launch
//!
//! **This file is the whole idea of the template.** Everything else in the crate is plumbing.
//!
//! During a launch window, every **buy** (a transfer out of the pool's vault) must pass four
//! checks. Outside the window, and for every transfer that is not a buy, nothing is checked.
//!
//! | Check | Stops | Error |
//! |---|---|---|
//! | the buy is at most `max_buy` tokens | one wallet vacuuming up the pool in a single swap | [`FairLaunchError::PerBuyCapExceeded`] |
//! | the buyer's balance afterwards is at most `max_wallet` | one token account accumulating a large share | [`FairLaunchError::MaxWalletExceeded`] |
//! | at most `max_buys_per_slot` buys land in the same slot | bundles: many buys packed into one block | [`FairLaunchError::TooManyBuysInSlot`] |
//! | the transaction's priority fee is at most `max_priority_micro_lamports` | winning the block by outbidding everyone | [`FairLaunchError::PriorityFeeTooHigh`] |
//!
//! ## What each check really guarantees
//!
//! * `max_wallet` is per **token account**, not per person: someone can open several accounts.
//!   The per-slot counter is what slows that down, because all their buys still share the slot's
//!   budget.
//! * The per-slot counter is per **mint**, not per buyer, so it is a launch-wide budget. A bundle
//!   of `max_buys_per_slot + 1` buys fails as a whole.
//! * The priority-fee check reads the transaction's `SetComputeUnitPrice` instruction. It sees the
//!   fee a legacy or v0 transaction **declares**. It cannot see a tip paid to a block builder through
//!   a plain transfer, so it limits ordinary fee wars, not private bundles.
//! * **It does not work on v1 transactions (SIMD-0385).** There the priority fee is a field of the
//!   message, and `ComputeBudget` instructions inside a v1 transaction are no-ops, so a hook
//!   reading them is bypassed (a v1 transaction can pay any fee and declare none) or fooled (it can
//!   declare a price the runtime ignores). Do not rely on this check where v1 transactions are
//!   accepted; the per-buy cap, per-account cap and per-slot budget do not depend on the format.
//! * Selling is never limited, and neither is moving tokens between wallets.
//!
//! ## One fact about Token-2022 this relies on
//!
//! Token-2022 moves the tokens **before** it calls the hook, so the buyer's balance the hook reads
//! is the balance *after* the buy.

use crate::error::FairLaunchError;

/// The launch window and its limits. Times are unix seconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Params {
    pub window_start: i64,
    /// The rule stops applying at this time.
    pub window_end: i64,
    pub max_buy: u64,
    pub max_wallet: u64,
    pub max_buys_per_slot: u32,
    /// `0` switches the priority-fee check off.
    pub max_priority_micro_lamports: u64,
}

impl Params {
    pub fn validate(&self) -> Result<(), FairLaunchError> {
        if self.window_end <= self.window_start
            || self.max_buy == 0
            || self.max_wallet == 0
            || self.max_buys_per_slot == 0
        {
            return Err(FairLaunchError::InvalidParams);
        }
        Ok(())
    }

    pub fn in_window(&self, now: i64) -> bool {
        (self.window_start..self.window_end).contains(&now)
    }
}

/// The facts about one buy the checks need.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Buy {
    pub amount: u64,
    /// The buyer's balance after the buy.
    pub wallet_balance_after: u64,
    /// Buys in this slot including this one.
    pub buys_in_slot: u32,
    /// The transaction's declared priority fee, if it declares one.
    pub priority_micro_lamports: Option<u64>,
}

/// The decision for one buy inside the window.
pub fn check_buy(params: &Params, buy: &Buy) -> Result<(), FairLaunchError> {
    if buy.amount > params.max_buy {
        return Err(FairLaunchError::PerBuyCapExceeded);
    }
    if buy.wallet_balance_after > params.max_wallet {
        return Err(FairLaunchError::MaxWalletExceeded);
    }
    if buy.buys_in_slot > params.max_buys_per_slot {
        return Err(FairLaunchError::TooManyBuysInSlot);
    }
    if params.max_priority_micro_lamports > 0
        && buy.priority_micro_lamports.unwrap_or(0) > params.max_priority_micro_lamports
    {
        return Err(FairLaunchError::PriorityFeeTooHigh);
    }
    Ok(())
}

/// The buy count for `slot` once one more buy lands, given the last recorded `(slot, count)`.
pub fn buys_in_slot_after(last_slot: u64, last_count: u32, slot: u64) -> u32 {
    if slot == last_slot {
        last_count.saturating_add(1)
    } else {
        1
    }
}

/// The price of a ComputeBudget `SetComputeUnitPrice` instruction (tag 3, then a `u64`), if `data`
/// is one.
pub fn priority_price(data: &[u8]) -> Option<u64> {
    match data.split_first() {
        Some((3, rest)) if rest.len() == 8 => Some(u64::from_le_bytes(rest.try_into().ok()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Params = Params {
        window_start: 100,
        window_end: 200,
        max_buy: 50,
        max_wallet: 120,
        max_buys_per_slot: 2,
        max_priority_micro_lamports: 1_000,
    };

    fn buy() -> Buy {
        Buy {
            amount: 50,
            wallet_balance_after: 120,
            buys_in_slot: 2,
            priority_micro_lamports: Some(1_000),
        }
    }

    #[test]
    fn a_buy_exactly_at_every_limit_passes() {
        assert_eq!(check_buy(&P, &buy()), Ok(()));
    }

    #[test]
    fn one_over_each_limit_is_refused_with_its_own_error() {
        let refused = |buy: Buy| check_buy(&P, &buy);
        assert_eq!(
            refused(Buy {
                amount: 51,
                ..buy()
            }),
            Err(FairLaunchError::PerBuyCapExceeded)
        );
        assert_eq!(
            refused(Buy {
                wallet_balance_after: 121,
                ..buy()
            }),
            Err(FairLaunchError::MaxWalletExceeded)
        );
        assert_eq!(
            refused(Buy {
                buys_in_slot: 3,
                ..buy()
            }),
            Err(FairLaunchError::TooManyBuysInSlot)
        );
        assert_eq!(
            refused(Buy {
                priority_micro_lamports: Some(1_001),
                ..buy()
            }),
            Err(FairLaunchError::PriorityFeeTooHigh)
        );
    }

    #[test]
    fn a_transaction_with_no_declared_fee_passes_the_fee_check() {
        assert_eq!(
            check_buy(
                &P,
                &Buy {
                    priority_micro_lamports: None,
                    ..buy()
                }
            ),
            Ok(())
        );
    }

    #[test]
    fn a_zero_cap_switches_the_fee_check_off() {
        let params = Params {
            max_priority_micro_lamports: 0,
            ..P
        };
        let loud = Buy {
            priority_micro_lamports: Some(u64::MAX),
            ..buy()
        };
        assert_eq!(check_buy(&params, &loud), Ok(()));
    }

    #[test]
    fn the_window_is_half_open() {
        assert!(!P.in_window(99));
        assert!(P.in_window(100));
        assert!(P.in_window(199));
        assert!(!P.in_window(200));
    }

    #[test]
    fn the_slot_counter_counts_within_a_slot_and_restarts_in_the_next() {
        assert_eq!(buys_in_slot_after(7, 1, 7), 2);
        assert_eq!(buys_in_slot_after(7, 2, 7), 3);
        assert_eq!(buys_in_slot_after(7, 9, 8), 1);
        assert_eq!(buys_in_slot_after(0, 0, 0), 1);
        assert_eq!(buys_in_slot_after(7, u32::MAX, 7), u32::MAX);
    }

    #[test]
    fn only_set_compute_unit_price_is_read_as_a_price() {
        let mut price = vec![3];
        price.extend_from_slice(&5_000u64.to_le_bytes());
        assert_eq!(priority_price(&price), Some(5_000));
        // SetComputeUnitLimit (tag 2) and malformed data are not prices.
        assert_eq!(priority_price(&[2, 1, 0, 0, 0]), None);
        assert_eq!(priority_price(&price[..8]), None);
        assert_eq!(priority_price(&[]), None);
    }

    #[test]
    fn validation_rejects_empty_windows_and_zero_limits() {
        assert_eq!(P.validate(), Ok(()));
        for bad in [
            Params {
                window_end: 100,
                ..P
            },
            Params { max_buy: 0, ..P },
            Params { max_wallet: 0, ..P },
            Params {
                max_buys_per_slot: 0,
                ..P
            },
        ] {
            assert_eq!(bad.validate(), Err(FairLaunchError::InvalidParams));
        }
    }
}
