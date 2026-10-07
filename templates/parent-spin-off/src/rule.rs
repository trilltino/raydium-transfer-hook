//! # The rule: a one-time spin-off allocation
//!
//! **This file is the whole idea of the template.** Everything else in the crate is plumbing.
//!
//! A project spins a **child** token off a **parent** token. Holders of the parent earn a share of
//! a fixed allocation of the child, in proportion to **how much parent they hold and for how
//! long**, over one window. The child tokens are deposited once, up front, into a vault only this
//! program can pay out of, and holders claim them separately; nothing is paid inside a transfer.
//!
//! The accounting is the balance-time index from `loyalty-rewards` (its [`Stream`] and
//! [`Holder`]), unchanged. This template adds exactly one rule on top:
//!
//! > **The allocation is funded once.** A second funding is refused, so the promise "these child
//! > tokens, over this window" cannot be quietly extended, diluted or topped up.
//!
//! (`loyalty-rewards` allows top-ups, because a loyalty programme is ongoing. A spin-off is an
//! event.)
//!
//! ## Transfer semantics, stated explicitly
//!
//! * **History stays with the historical holder.** What a holder has earned up to the moment they
//!   sell is theirs to claim, whoever holds the parent afterwards.
//! * **The future follows the parent balance.** From that moment the buyer, if registered,
//!   accrues on the balance they now hold.
//! * **Only registered accounts earn,** and the pool's vault can never register, so the pool does
//!   not collect the holders' share.
//!
//! ## What this does not stop
//!
//! * A spin-off cannot be paid to holders who never register. Registration is explicit (about
//!   0.0015 SOL of rent each).
//! * Burning parent tokens is invisible to a hook; see `loyalty-rewards` for how earnings are
//!   capped.
//! * The child token must be a plain token. A child with a Transfer Hook of its own (a nested hook)
//!   is refused: its claims would need extra accounts this program does not forward.
//! * The window is chosen by whoever funds it, once. Fund it from an account you trust to choose
//!   it, or have the program's upgrade authority revoked.

pub use loyalty_rewards_hook::rule::{Holder, Stream};

use crate::error::SpinOffError;

/// The funding rule: an allocation can be funded once. `stream_rate` is the stream's current
/// per-second rate, which is zero until the first funding.
pub fn check_funding(stream_rate: u64) -> Result<(), SpinOffError> {
    if stream_rate != 0 {
        return Err(SpinOffError::AlreadyFunded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10,000-token allocation streamed over 100 seconds from t = 0.
    fn funded() -> Stream {
        let mut stream = Stream::default();
        stream.fund(0, 10_000, 100).unwrap();
        stream
    }

    #[test]
    fn the_first_funding_is_allowed_and_a_second_is_refused() {
        let mut stream = Stream::default();
        assert_eq!(check_funding(stream.rate), Ok(()));
        stream.fund(0, 10_000, 100).unwrap();
        assert_eq!(check_funding(stream.rate), Err(SpinOffError::AlreadyFunded));
        // Even after the window has ended.
        stream.advance(10_000).unwrap();
        assert_eq!(check_funding(stream.rate), Err(SpinOffError::AlreadyFunded));
    }

    #[test]
    fn history_stays_with_the_seller_and_the_future_follows_the_buyer() {
        let mut stream = funded();
        let mut seller = Holder::register(&mut stream, 1_000).unwrap();
        let mut buyer = Holder::register(&mut stream, 0).unwrap();

        // At t = 50 the seller sells everything to the buyer.
        stream.advance(50).unwrap();
        seller.on_balance_change(&mut stream, 1_000, 0).unwrap();
        buyer.on_balance_change(&mut stream, 0, 1_000).unwrap();
        assert_eq!(seller.earned, 5_000, "the seller keeps what they earned");
        assert_eq!(buyer.earned, 0);

        stream.advance(100).unwrap();
        assert_eq!(seller.claim(&mut stream, 0), Ok(5_000));
        assert_eq!(buyer.claim(&mut stream, 1_000), Ok(5_000));
    }

    #[test]
    fn the_allocation_is_never_over_paid() {
        let mut stream = funded();
        let mut holders: Vec<Holder> = [300u64, 500, 200]
            .iter()
            .map(|b| Holder::register(&mut stream, *b).unwrap())
            .collect();
        stream.advance(40).unwrap();
        // The first holder moves 100 to the third halfway through.
        holders[0].on_balance_change(&mut stream, 300, 200).unwrap();
        holders[2].on_balance_change(&mut stream, 200, 300).unwrap();
        stream.advance(100).unwrap();
        let balances = [200u64, 500, 300];
        let paid: u64 = holders
            .iter_mut()
            .zip(balances)
            .map(|(holder, balance)| holder.claim(&mut stream, balance).unwrap())
            .sum();
        assert!(paid <= 10_000, "paid {paid} of 10,000");
        assert!(paid >= 9_990, "rounding dust should be tiny, paid {paid}");
    }

    #[test]
    fn nothing_accrues_before_the_allocation_is_funded() {
        let mut stream = Stream::default();
        let mut holder = Holder::register(&mut stream, 1_000).unwrap();
        stream.advance(500).unwrap();
        assert_eq!(holder.claim(&mut stream, 1_000), Ok(0));
    }
}
