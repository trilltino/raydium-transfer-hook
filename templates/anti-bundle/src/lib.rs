//! # Anti-bundle
//!
//! A Token-2022 Transfer Hook that gives each slot a small budget of **buys** from recognised
//! venues, so a bundle of many buys in one block is refused as a whole.
//!
//! **Read [`rule`] first.** It is the whole idea: [`rule::Params`], [`rule::is_buy`] and one
//! decision, [`rule::check_buy`]. The rest of the crate is plumbing:
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the per-slot budget and what counts as a buy (pure, unit-tested) |
//! | [`state`] | the per-mint config (budget, venues) and the slot counter |
//! | [`instruction`] | the setup instruction |
//! | `processor` | `Initialize` and `Execute`, built on [`hook_kit`] |
//! | [`error`] | error codes from `0xD001` |

pub mod error;
pub mod instruction;
pub mod processor;
pub mod rule;
pub mod state;

pub use processor::process_instruction;

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
