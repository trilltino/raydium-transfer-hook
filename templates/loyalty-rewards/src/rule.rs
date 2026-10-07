//! # The rule: rewards for balance x time
//!
//! **This file is the whole idea of the template.** Everything else in the crate is plumbing.
//!
//! A reward pool is funded with an `amount` to be paid out evenly over `duration` seconds. While
//! it pays out, every second's share goes to the registered holders **in proportion to their
//! balance**. So a holder's earnings are their balance times the time they held it, relative to
//! everyone else's: the loyal holder earns more than the one who arrived late, and buying right
//! before a payout earns nothing, because nothing is paid out at an instant.
//!
//! ## How it stays cheap: one index
//!
//! The pool keeps a single running number, the **index**: the reward earned so far *per unit of
//! balance*. Every second the index grows by `rate / eligible_supply`. A holder keeps the index
//! value they were last settled at, so their unsettled earnings are always
//! `balance * (index - index_paid)`, with no loop over holders. A transfer settles the two
//! holders it touches. That is why this is cheap enough to run inside a swap.
//!
//! ```text
//! index
//!   ^                     .-----   funded stream stops at `period_finish`
//!   |                 .-'
//!   |             .-'              slope = rate / eligible_supply
//!   |         .-'
//!   +-----.-'------------------> time
//!      funded
//! ```
//!
//! ## Which balances count
//!
//! Only **registered** token accounts earn, and `eligible_supply` is the sum of their balances.
//! The pool's own vault can never register, so the pool does not earn the holders' rewards. An
//! unregistered account earns nothing and is not counted.
//!
//! ## What this does not stop
//!
//! * Burning a token is not a transfer, so the hook never sees it. A holder's earnings are capped
//!   by their *actual* balance when they settle, and the eligible supply is corrected at their next
//!   transfer, but until then burned tokens still dilute everyone else a little.
//! * While `eligible_supply` is zero, the stream's rewards for that time are not paid to anyone;
//!   they stay in the vault.
//!
//! All arithmetic is integer, `u128` internally, and rounds **down**, so the vault can never owe
//! more than it was funded with.

use crate::error::LoyaltyError;

/// Fixed-point scale of the index.
pub const PRECISION: u128 = 1_000_000_000_000;

/// The longest reward period accepted: about ten years.
pub const MAX_DURATION: u32 = 315_360_000;

/// The pool's running accounts. Stored in the global account.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stream {
    /// Reward tokens paid out per second while the period lasts.
    pub rate: u64,
    pub period_finish: i64,
    /// The last time the index was brought up to date.
    pub last_update: i64,
    /// Reward earned so far per unit of balance, scaled by [`PRECISION`].
    pub index: u128,
    /// The sum of the balances of all registered accounts.
    pub eligible_supply: u64,
}

impl Stream {
    /// Bring the index up to `now`.
    pub fn advance(&mut self, now: i64) -> Result<(), LoyaltyError> {
        let until = now.min(self.period_finish);
        if until > self.last_update && self.eligible_supply > 0 {
            let seconds = (until - self.last_update) as u128;
            let growth = (self.rate as u128)
                .checked_mul(seconds)
                .and_then(|v| v.checked_mul(PRECISION))
                .ok_or(LoyaltyError::MathOverflow)?
                / self.eligible_supply as u128;
            self.index = self
                .index
                .checked_add(growth)
                .ok_or(LoyaltyError::MathOverflow)?;
        }
        self.last_update = self.last_update.max(now);
        Ok(())
    }

    /// Add `amount` reward tokens to be paid out over `duration` seconds from `now`. Whatever the
    /// current period has not yet paid out is rolled into the new one.
    pub fn fund(&mut self, now: i64, amount: u64, duration: u32) -> Result<(), LoyaltyError> {
        if amount == 0 {
            return Err(LoyaltyError::ZeroAmount);
        }
        if duration == 0 || duration > MAX_DURATION {
            return Err(LoyaltyError::InvalidDuration);
        }
        self.advance(now)?;
        let remaining = (self.period_finish - now).max(0) as u128;
        let leftover = remaining * self.rate as u128;
        let total = (amount as u128)
            .checked_add(leftover)
            .ok_or(LoyaltyError::MathOverflow)?;
        let rate =
            u64::try_from(total / duration as u128).map_err(|_| LoyaltyError::MathOverflow)?;
        if rate == 0 {
            // The reward is too small to pay out even one unit per second.
            return Err(LoyaltyError::ZeroAmount);
        }
        self.rate = rate;
        self.period_finish = now
            .checked_add(duration as i64)
            .ok_or(LoyaltyError::MathOverflow)?;
        Ok(())
    }

    /// Replace `old` by `new` in the eligible supply.
    fn set_eligible(&mut self, old: u64, new: u64) -> Result<(), LoyaltyError> {
        self.eligible_supply = self
            .eligible_supply
            .checked_sub(old)
            .and_then(|v| v.checked_add(new))
            .ok_or(LoyaltyError::MathOverflow)?;
        Ok(())
    }
}

/// One registered token account's place in the stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Holder {
    /// The balance this record last counted in the eligible supply.
    pub checkpoint: u64,
    /// The index value this holder was last settled at.
    pub index_paid: u128,
    /// Earned and settled, not yet claimed.
    pub earned: u64,
}

impl Holder {
    /// Register an account holding `balance`. `stream` must already be advanced to now.
    pub fn register(stream: &mut Stream, balance: u64) -> Result<Self, LoyaltyError> {
        stream.set_eligible(0, balance)?;
        Ok(Self {
            checkpoint: balance,
            index_paid: stream.index,
            earned: 0,
        })
    }

    /// Settle what accrued since the last settlement, for a holder whose actual balance up to now
    /// was `balance_before`. `stream` must already be advanced to now.
    ///
    /// The earning balance is the smaller of the recorded and the actual one, so tokens that left
    /// without a transfer (a burn) do not keep earning.
    pub fn settle(&mut self, stream: &Stream, balance_before: u64) -> Result<(), LoyaltyError> {
        let earning = self.checkpoint.min(balance_before) as u128;
        let accrued = earning
            .checked_mul(stream.index - self.index_paid)
            .ok_or(LoyaltyError::MathOverflow)?
            / PRECISION;
        self.earned = u64::try_from(accrued)
            .ok()
            .and_then(|a| self.earned.checked_add(a))
            .ok_or(LoyaltyError::MathOverflow)?;
        self.index_paid = stream.index;
        Ok(())
    }

    /// A transfer changed the balance from `before` to `after`: settle, then count the new balance.
    pub fn on_balance_change(
        &mut self,
        stream: &mut Stream,
        before: u64,
        after: u64,
    ) -> Result<(), LoyaltyError> {
        self.settle(stream, before)?;
        stream.set_eligible(self.checkpoint, after)?;
        self.checkpoint = after;
        Ok(())
    }

    /// Everything earned up to now, for a holder whose actual balance is `balance_now`. Resets
    /// `earned`. `stream` must already be advanced to now.
    pub fn claim(&mut self, stream: &mut Stream, balance_now: u64) -> Result<u64, LoyaltyError> {
        self.settle(stream, balance_now)?;
        // Re-count the balance too, so a burn since the last transfer stops diluting others.
        stream.set_eligible(self.checkpoint, balance_now)?;
        self.checkpoint = balance_now;
        Ok(std::mem::take(&mut self.earned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream paying 1,000 over 100 seconds starting at t = 0.
    fn funded() -> Stream {
        let mut stream = Stream::default();
        stream.fund(0, 1_000, 100).unwrap();
        stream
    }

    #[test]
    fn a_lone_holder_earns_the_whole_stream() {
        let mut stream = funded();
        let mut holder = Holder::register(&mut stream, 500).unwrap();
        stream.advance(100).unwrap();
        assert_eq!(holder.claim(&mut stream, 500), Ok(1_000));
    }

    #[test]
    fn rewards_split_in_proportion_to_balance() {
        let mut stream = funded();
        let mut a = Holder::register(&mut stream, 300).unwrap();
        let mut b = Holder::register(&mut stream, 100).unwrap();
        stream.advance(100).unwrap();
        assert_eq!(a.claim(&mut stream, 300), Ok(750));
        assert_eq!(b.claim(&mut stream, 100), Ok(250));
    }

    #[test]
    fn the_loyal_holder_earns_more_than_the_latecomer() {
        // Equal balances, but B only arrives halfway through.
        let mut stream = funded();
        let mut a = Holder::register(&mut stream, 100).unwrap();
        stream.advance(50).unwrap();
        let mut b = Holder::register(&mut stream, 100).unwrap();
        stream.advance(100).unwrap();
        let (earned_a, earned_b) = (
            a.claim(&mut stream, 100).unwrap(),
            b.claim(&mut stream, 100).unwrap(),
        );
        // First half: A alone earns 500. Second half: 500 split evenly.
        assert_eq!((earned_a, earned_b), (750, 250));
    }

    #[test]
    fn buying_just_before_the_end_earns_almost_nothing() {
        let mut stream = funded();
        let mut a = Holder::register(&mut stream, 100).unwrap();
        stream.advance(99).unwrap();
        // A whale arrives one second before the stream ends.
        let mut whale = Holder::register(&mut stream, 9_900).unwrap();
        stream.advance(100).unwrap();
        let earned_whale = whale.claim(&mut stream, 9_900).unwrap();
        let earned_a = a.claim(&mut stream, 100).unwrap();
        // The whale holds 99% of the supply for 1% of the time: 10 tokens of the last second.
        assert_eq!(earned_whale, 9);
        assert!(earned_a >= 990);
        assert!(earned_a + earned_whale <= 1_000);
    }

    #[test]
    fn a_transfer_settles_both_sides_and_moves_the_eligible_supply() {
        let mut stream = funded();
        let mut a = Holder::register(&mut stream, 100).unwrap();
        let mut b = Holder::register(&mut stream, 100).unwrap();
        stream.advance(50).unwrap();
        // A sends 60 to B at t = 50.
        a.on_balance_change(&mut stream, 100, 40).unwrap();
        b.on_balance_change(&mut stream, 100, 160).unwrap();
        assert_eq!(stream.eligible_supply, 200);
        stream.advance(100).unwrap();
        // First half 250 each; second half 500 split 40:160.
        assert_eq!(a.claim(&mut stream, 40), Ok(250 + 100));
        assert_eq!(b.claim(&mut stream, 160), Ok(250 + 400));
    }

    #[test]
    fn nothing_accrues_after_the_period_ends() {
        let mut stream = funded();
        let mut holder = Holder::register(&mut stream, 10).unwrap();
        stream.advance(1_000).unwrap();
        assert_eq!(holder.claim(&mut stream, 10), Ok(1_000));
        stream.advance(5_000).unwrap();
        assert_eq!(holder.claim(&mut stream, 10), Ok(0));
    }

    #[test]
    fn funding_again_rolls_the_unpaid_part_into_the_new_period() {
        let mut stream = funded();
        stream.advance(50).unwrap();
        // 500 remains; add 500 over 100 more seconds.
        stream.fund(50, 500, 100).unwrap();
        assert_eq!(stream.rate, 10);
        assert_eq!(stream.period_finish, 150);
    }

    #[test]
    fn rewards_for_a_time_with_no_eligible_balance_are_not_paid_to_anyone() {
        let mut stream = funded();
        stream.advance(60).unwrap();
        // The first holder arrives at t = 60: only the last 40 seconds can be earned.
        let mut holder = Holder::register(&mut stream, 10).unwrap();
        stream.advance(100).unwrap();
        assert_eq!(holder.claim(&mut stream, 10), Ok(400));
    }

    #[test]
    fn a_burned_balance_stops_earning_at_the_next_settlement() {
        let mut stream = funded();
        let mut holder = Holder::register(&mut stream, 100).unwrap();
        stream.advance(100).unwrap();
        // 90 of the 100 tokens were burned during the period; only the actual 10 count.
        assert_eq!(holder.claim(&mut stream, 10), Ok(100));
        assert_eq!(
            stream.eligible_supply, 10,
            "the burn is corrected at the claim"
        );
    }

    #[test]
    fn payouts_never_exceed_the_funded_amount_even_with_awkward_numbers() {
        let mut stream = Stream::default();
        stream.fund(0, 1_000_003, 7).unwrap();
        let mut holders: Vec<Holder> = [3u64, 5, 11, 17]
            .iter()
            .map(|b| Holder::register(&mut stream, *b).unwrap())
            .collect();
        stream.advance(7).unwrap();
        let paid: u64 = holders
            .iter_mut()
            .zip([3u64, 5, 11, 17])
            .map(|(h, b)| h.claim(&mut stream, b).unwrap())
            .sum();
        assert!(paid <= 1_000_003 - (1_000_003 % 7), "paid {paid}");
    }

    #[test]
    fn invalid_funding_is_rejected() {
        let mut stream = Stream::default();
        assert_eq!(stream.fund(0, 0, 10), Err(LoyaltyError::ZeroAmount));
        assert_eq!(stream.fund(0, 10, 0), Err(LoyaltyError::InvalidDuration));
        assert_eq!(
            stream.fund(0, 10, MAX_DURATION + 1),
            Err(LoyaltyError::InvalidDuration)
        );
        // 5 tokens over 10 seconds is under one unit per second.
        assert_eq!(stream.fund(0, 5, 10), Err(LoyaltyError::ZeroAmount));
    }

    #[test]
    fn the_arithmetic_cannot_overflow_at_the_extremes() {
        let mut stream = Stream::default();
        stream.fund(0, u64::MAX, 1).unwrap();
        let mut holder = Holder::register(&mut stream, 1).unwrap();
        stream.advance(1).unwrap();
        assert_eq!(holder.claim(&mut stream, 1), Ok(u64::MAX));
    }
}
