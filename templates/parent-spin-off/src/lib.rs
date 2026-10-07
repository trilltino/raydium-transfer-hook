//! # Parent / spin-off
//!
//! A Token-2022 Transfer Hook that pays holders of a **parent** token a one-time allocation of a
//! **child** token, in proportion to balance x time held.
//!
//! **Read [`rule`] first.** The accounting is `loyalty-rewards`' balance-time index; the rule this
//! template adds is one line: the allocation is funded once. The rest is composition:
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the single-funding rule and the stated transfer semantics (unit-tested) |
//! | `instruction` | the instructions, shared with `loyalty-rewards` (same layouts) |
//! | `processor` | the single-funding guard in front of `loyalty-rewards`' processor |
//! | [`error`] | error codes from `0xE001` |

pub mod error;
pub mod rule;

/// The instructions are `loyalty-rewards`': `Initialize`, `Register`, `Fund`, `Claim`.
pub mod instruction {
    pub use loyalty_rewards_hook::instruction::*;
}

/// The accounts are `loyalty-rewards`' too: the global, one record per registered token account.
pub mod state {
    pub use loyalty_rewards_hook::state::*;
}

mod processor;

pub use processor::process_instruction;

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
