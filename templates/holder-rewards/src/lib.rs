//! # Holder rewards
//!
//! A Token-2022 Transfer Hook that pays holders a reward stream in another token (for example the
//! quote token), in proportion to **balance x time held**.
//!
//! **Read [`rule`] first.** It is the whole idea: a [`rule::Stream`] with one running index, and a
//! [`rule::Holder`] that settles against it. The rest of the crate is plumbing:
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the reward accounting (pure, unit-tested) |
//! | [`state`] | the global account (the stream) and one record per registered token account |
//! | [`instruction`] | `Initialize`, `Register`, `Fund`, `Claim` |
//! | `processor` | the instructions and `Execute`, built on [`hook_kit`] |
//! | [`error`] | error codes from `0xC001` |

pub mod error;
pub mod instruction;
pub mod processor;
pub mod rule;
pub mod state;

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
