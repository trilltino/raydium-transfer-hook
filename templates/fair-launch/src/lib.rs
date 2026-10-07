//! # Fair launch
//!
//! A Token-2022 Transfer Hook that limits **buys** during a launch window: a per-buy cap, a
//! per-wallet cap, a per-slot buy budget (against bundles), and a priority-fee cap (against fee
//! wars).
//!
//! **Read [`rule`] first.** It is the whole idea: [`rule::Params`] and one decision,
//! [`rule::check_buy`]. The rest of the crate is plumbing:
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the four checks and the window (pure, unit-tested) |
//! | [`config`] | the per-mint config account and the slot counter |
//! | [`instruction`] | the setup instruction |
//! | `processor` | `Initialize` and `Execute`, built on [`hook_kit`] |
//! | [`error`] | error codes from `0xB001` |

pub mod config;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod rule;

pub use processor::process_instruction;

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
