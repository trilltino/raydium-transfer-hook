//! # The rule: a creator allocation that vests
//!
//! **This file is the whole idea of the template.** Everything else in the crate is plumbing.
//!
//! The creator's allocation sits in one dedicated token account. A [`Schedule`] says how much of it
//! is still locked at any moment. Whenever tokens leave that account, the balance that remains
//! must be at least the locked amount. Nothing else is restricted: anyone else can transfer
//! freely, and the creator can receive freely.
//!
//! ```text
//! locked
//!   ^
//!   |#########                      locked_total
//!   |        #  .
//!   |        #     .               linear unlock from `start` to `end`,
//!   |        #        .            but nothing before `cliff`
//!   |        #           .
//!   +--------+-------------+--> time
//!          cliff           end
//! ```
//!
//! ## Why moving wallets does not help
//!
//! The floor belongs to the **token account**, not to a wallet. Locked tokens cannot leave that
//! account at all, so there is nothing to move. Handing the account to another owner keeps the
//! floor in place, and so does letting a delegate spend it (both are tested in
//! `tests/creator_commitment.rs`). Only the part above the floor is ever transferable, and that
//! part carries no history: it is ordinary, fungible tokens.
//!
//! ## What this does not stop
//!
//! * Burning is not a transfer, so the hook never sees it. A creator can burn their own locked
//!   tokens (tested); that only hurts the creator, whose balance is then below the floor until the
//!   schedule ends.
//! * The program's upgrade authority can replace this rule. Disclose it or revoke it.
//!
//! ## One fact about Token-2022 this relies on
//!
//! Token-2022 moves the tokens **before** it calls the hook, so the balance the hook reads from
//! the source account is the balance *after* the transfer. The floor check compares that
//! post-transfer balance with the locked amount.

use crate::error::CommitmentError;

/// How much of the allocation unlocks, and when. Times are unix seconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Schedule {
    /// Tokens locked at the start. The creator's account must hold at least this when the
    /// commitment is set up.
    pub locked_total: u64,
    pub start: i64,
    /// Nothing unlocks before this time. After it, the amount unlocked jumps to what the linear
    /// schedule has reached.
    pub cliff: i64,
    /// Everything is unlocked from this time.
    pub end: i64,
}

impl Schedule {
    /// A schedule must lock something, run forward in time, and put the cliff inside it.
    pub fn validate(&self) -> Result<(), CommitmentError> {
        if self.locked_total == 0 {
            return Err(CommitmentError::ZeroLockedAmount);
        }
        if self.end <= self.start || self.cliff < self.start || self.cliff > self.end {
            return Err(CommitmentError::InvalidSchedule);
        }
        Ok(())
    }

    /// Tokens still locked at `now`.
    ///
    /// Rounds in the creator's disfavour: the unlocked amount is rounded down, so the locked
    /// amount is rounded up, and a schedule never unlocks a token early.
    #[must_use]
    pub fn locked_at(&self, now: i64) -> u64 {
        if now < self.cliff {
            return self.locked_total;
        }
        if now >= self.end {
            return 0;
        }
        // `start <= cliff <= now < end`, so both differences are positive. `abs_diff` computes
        // them exactly even when the endpoints are `i64::MIN` and `i64::MAX`, where a plain
        // subtraction would overflow; each fits a `u64`, so the product fits a `u128`.
        let elapsed = u128::from(now.abs_diff(self.start));
        let duration = u128::from(self.end.abs_diff(self.start));
        let unlocked = u128::from(self.locked_total) * elapsed / duration;
        // `unlocked < locked_total` because `elapsed < duration`.
        self.locked_total - u64::try_from(unlocked).unwrap_or(self.locked_total)
    }
}

/// The decision, for a transfer **out of** the creator's account: allow it only if the balance left
/// afterwards still covers what is locked.
///
/// `balance_after` is the account's post-transfer balance.
pub fn check_outgoing(
    schedule: &Schedule,
    now: i64,
    balance_after: u64,
) -> Result<(), CommitmentError> {
    if balance_after < schedule.locked_at(now) {
        return Err(CommitmentError::VestingFloorBreached);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Schedule = Schedule {
        locked_total: 1_000,
        start: 100,
        cliff: 200,
        end: 1_100,
    };

    #[test]
    fn everything_is_locked_before_the_cliff() {
        for now in [i64::MIN, 0, 99, 100, 150, 199] {
            assert_eq!(S.locked_at(now), 1_000, "at {now}");
        }
    }

    #[test]
    fn unlocking_is_linear_from_the_start_once_the_cliff_passes() {
        // At the cliff, 100 of 1000 seconds have elapsed: 10% unlocked.
        assert_eq!(S.locked_at(200), 900);
        assert_eq!(S.locked_at(600), 500);
        assert_eq!(S.locked_at(1_099), 1);
    }

    #[test]
    fn everything_is_unlocked_from_the_end() {
        for now in [1_100, 1_101, i64::MAX] {
            assert_eq!(S.locked_at(now), 0, "at {now}");
        }
    }

    #[test]
    fn locked_amount_rounds_up_so_no_token_unlocks_early() {
        let s = Schedule {
            locked_total: 10,
            start: 0,
            cliff: 0,
            end: 3,
        };
        // 1/3 of 10 = 3.33 unlocked -> 3, so 7 stay locked.
        assert_eq!(s.locked_at(1), 7);
        // 2/3 of 10 = 6.66 unlocked -> 6, so 4 stay locked.
        assert_eq!(s.locked_at(2), 4);
    }

    #[test]
    fn the_locked_amount_never_increases_and_never_exceeds_the_total() {
        let mut previous = S.locked_total;
        for now in (0..1_300).step_by(7) {
            let locked = S.locked_at(now);
            assert!(locked <= previous, "locked grew at {now}");
            assert!(locked <= S.locked_total);
            previous = locked;
        }
    }

    #[test]
    fn the_arithmetic_cannot_overflow_at_the_extremes() {
        let s = Schedule {
            locked_total: u64::MAX,
            start: i64::MIN / 2,
            cliff: i64::MIN / 2,
            end: i64::MAX / 2,
        };
        let mid = s.locked_at(0);
        assert!(mid > 0 && mid < u64::MAX);
        assert_eq!(s.locked_at(i64::MAX / 2), 0);
    }

    #[test]
    fn a_schedule_spanning_the_whole_i64_range_is_evaluated_exactly() {
        // `now - start` and `end - start` overflow an `i64` here; the answer is still exact.
        let s = Schedule {
            locked_total: 1_000_000,
            start: i64::MIN,
            cliff: i64::MIN,
            end: i64::MAX,
        };
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.locked_at(i64::MIN), 1_000_000);
        // Just under half has elapsed at -1, so one more token than half stays locked.
        assert_eq!(s.locked_at(-1), 500_001);
        assert_eq!(s.locked_at(0), 500_000);
        assert_eq!(s.locked_at(i64::MAX - 1), 1);
        assert_eq!(s.locked_at(i64::MAX), 0);
        let big = Schedule {
            locked_total: u64::MAX,
            ..s
        };
        assert_eq!(big.locked_at(i64::MAX - 1), 1);
        assert!(big.locked_at(0) > 0 && big.locked_at(0) < u64::MAX);
        // Never increases, even at the extremes.
        let mut previous = u64::MAX;
        for now in [
            i64::MIN,
            i64::MIN + 1,
            -1 << 62,
            -1,
            0,
            1,
            1 << 62,
            i64::MAX - 1,
            i64::MAX,
        ] {
            let locked = big.locked_at(now);
            assert!(locked <= previous, "grew at {now}");
            previous = locked;
        }
    }

    #[test]
    fn validation_rejects_empty_backwards_and_misplaced_cliff_schedules() {
        assert_eq!(S.validate(), Ok(()));
        let bad = |s: Schedule| s.validate();
        assert_eq!(
            bad(Schedule {
                locked_total: 0,
                ..S
            }),
            Err(CommitmentError::ZeroLockedAmount)
        );
        assert_eq!(
            bad(Schedule { end: 100, ..S }),
            Err(CommitmentError::InvalidSchedule)
        );
        assert_eq!(
            bad(Schedule { cliff: 99, ..S }),
            Err(CommitmentError::InvalidSchedule)
        );
        assert_eq!(
            bad(Schedule { cliff: 1_101, ..S }),
            Err(CommitmentError::InvalidSchedule)
        );
    }

    #[test]
    fn selling_down_to_exactly_the_floor_is_allowed_and_one_below_is_not() {
        // Before the cliff the floor is the full 1000.
        assert_eq!(check_outgoing(&S, 150, 1_000), Ok(()));
        assert_eq!(
            check_outgoing(&S, 150, 999),
            Err(CommitmentError::VestingFloorBreached)
        );
        // Halfway through, 500 is locked.
        assert_eq!(check_outgoing(&S, 600, 500), Ok(()));
        assert_eq!(
            check_outgoing(&S, 600, 499),
            Err(CommitmentError::VestingFloorBreached)
        );
        // After the end nothing is locked, even down to zero.
        assert_eq!(check_outgoing(&S, 2_000, 0), Ok(()));
    }
}
