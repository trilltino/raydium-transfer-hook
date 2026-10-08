//! # Creator commitment
//!
//! A Token-2022 Transfer Hook that makes a creator's allocation vest: the balance of one dedicated
//! token account may not fall below the amount still locked by a cliff-and-linear schedule.
//!
//! **Read [`rule`] first.** It is the whole idea: a [`rule::Schedule`] and one decision,
//! [`rule::check_outgoing`]. The rest of the crate is plumbing:
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the vesting schedule and the decision (pure, unit-tested) |
//! | [`config`] | the per-mint config account |
//! | [`instruction`] | the setup instruction |
//! | `processor` | `Initialize` and `Execute`, built on [`hook_kit`] |
//! | [`error`] | error codes from `0xA001` |

pub mod config;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod rule;

#[cfg(test)]
mod fixture_tests;

pub use processor::process_instruction;

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
