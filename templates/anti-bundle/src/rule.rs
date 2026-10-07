//! # The rule: a per-slot budget on buys
//!
//! **This file is the whole idea of the template.** Everything else in the crate is plumbing.
//!
//! A **bundle** packs many buys into one block so a single actor takes the opening of a launch
//! before anyone else can react. This rule gives each slot a small budget of buys: at most
//! `max_buys_per_slot` buys from the recognised venues may land in the same slot, and the next
//! buy in that slot is refused, which fails the whole transaction that contained it.
//!
//! A **buy** is a transfer whose source is one of the configured venue vaults (the pool's vault of
//! this token). Ordinary wallet-to-wallet transfers, sells into a pool, and anything else are never
//! counted and never refused: counting transfers in general would punish normal use, not bundlers.
//!
//! ```text
//! slot 100:  buy buy buy buy        <- with max_buys_per_slot = 3 the 4th is refused
//! slot 101:  buy buy                <- the budget starts over
//! ```
//!
//! ## What this does and does not guarantee
//!
//! * The budget is per **mint and slot**, shared by every venue and every buyer. When it is used
//!   up, an honest buyer in the same slot is refused too. That is the price of stopping a bundle.
//! * It does **not** limit one buyer across slots, or one buyer using many accounts across slots.
//!   (`fair-launch` adds a per-account balance cap for that.)
//! * It only recognises the venues it was configured with. A new pool, or a route through a venue
//!   that was not configured, is not counted.
//! * It counts transfers, not swaps: a swap that buys from one venue counts once.

use crate::error::AntiBundleError;

/// The most venues one configuration can recognise.
pub const MAX_VENUES: usize = 4;

/// The budget, and until when it applies. Times are unix seconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Params {
    /// The rule stops applying at this time. `0` means it never stops.
    pub active_until: i64,
    pub max_buys_per_slot: u16,
}

impl Params {
    pub fn validate(&self) -> Result<(), AntiBundleError> {
        if self.max_buys_per_slot == 0 {
            return Err(AntiBundleError::InvalidParams);
        }
        Ok(())
    }

    pub fn is_active(&self, now: i64) -> bool {
        self.active_until == 0 || now < self.active_until
    }
}

/// Whether a transfer out of `source` is a buy: it comes from one of the recognised venues.
pub fn is_buy<K: PartialEq>(source: &K, venues: &[K]) -> bool {
    venues.contains(source)
}

/// The number of buys in `slot` once one more buy lands, given the last recorded `(slot, count)`.
pub fn buys_in_slot_after(last_slot: u64, last_count: u16, slot: u64) -> u16 {
    if slot == last_slot {
        last_count.saturating_add(1)
    } else {
        1
    }
}

/// The decision for one buy, given the count including it.
pub fn check_buy(params: &Params, buys_in_slot: u16) -> Result<(), AntiBundleError> {
    if buys_in_slot > params.max_buys_per_slot {
        return Err(AntiBundleError::TooManyBuysInSlot);
    }
    Ok(())
}

/// Venues must be at least one, at most [`MAX_VENUES`], and distinct.
pub fn validate_venues<K: PartialEq>(venues: &[K]) -> Result<(), AntiBundleError> {
    if venues.is_empty() || venues.len() > MAX_VENUES {
        return Err(AntiBundleError::InvalidVenues);
    }
    for (i, venue) in venues.iter().enumerate() {
        if venues[..i].contains(venue) {
            return Err(AntiBundleError::InvalidVenues);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Params = Params {
        active_until: 1_000,
        max_buys_per_slot: 3,
    };

    #[test]
    fn the_budget_is_spent_buy_by_buy_within_a_slot() {
        let mut count = 0;
        for expected in 1..=3 {
            count = buys_in_slot_after(7, count, 7);
            assert_eq!(count, expected);
            assert_eq!(check_buy(&P, count), Ok(()));
        }
        assert_eq!(
            check_buy(&P, buys_in_slot_after(7, 3, 7)),
            Err(AntiBundleError::TooManyBuysInSlot)
        );
    }

    #[test]
    fn a_new_slot_starts_a_fresh_budget() {
        assert_eq!(buys_in_slot_after(7, 3, 8), 1);
        assert_eq!(buys_in_slot_after(0, 0, 0), 1);
        assert_eq!(buys_in_slot_after(7, u16::MAX, 7), u16::MAX);
        assert_eq!(check_buy(&P, buys_in_slot_after(7, 3, 8)), Ok(()));
    }

    #[test]
    fn only_a_recognised_venue_makes_a_transfer_a_buy() {
        let venues = ["pool-a", "pool-b"];
        assert!(is_buy(&"pool-a", &venues));
        assert!(is_buy(&"pool-b", &venues));
        assert!(!is_buy(&"wallet", &venues));
        assert!(!is_buy(&"pool-a", &[] as &[&str]));
    }

    #[test]
    fn a_zero_active_until_never_expires() {
        let forever = Params {
            active_until: 0,
            ..P
        };
        assert!(forever.is_active(i64::MAX));
        assert!(P.is_active(999));
        assert!(!P.is_active(1_000));
    }

    #[test]
    fn validation_rejects_a_zero_budget_and_bad_venue_lists() {
        assert_eq!(P.validate(), Ok(()));
        assert_eq!(
            Params {
                max_buys_per_slot: 0,
                ..P
            }
            .validate(),
            Err(AntiBundleError::InvalidParams)
        );
        assert_eq!(validate_venues(&["a", "b"]), Ok(()));
        assert_eq!(
            validate_venues::<&str>(&[]),
            Err(AntiBundleError::InvalidVenues)
        );
        assert_eq!(
            validate_venues(&["a", "a"]),
            Err(AntiBundleError::InvalidVenues)
        );
        assert_eq!(
            validate_venues(&["a", "b", "c", "d", "e"]),
            Err(AntiBundleError::InvalidVenues)
        );
    }
}
