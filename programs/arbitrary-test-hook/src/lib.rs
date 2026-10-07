#![deny(unsafe_code)]
//! # Arbitrary test hook
//!
//! A deliberately unrelated, third-party-style Transfer Hook. It exists to prove that the
//! framework works for hooks it knows nothing about: this crate has its own program id, its
//! own PDA scheme, its own init instruction, its own error codes and its own rule, and it does
//! not depend on the reference hook or on any Raydium crate. Nothing in the SDK or the Raydium
//! builders is aware of it.
//!
//! ## Rule
//!
//! At most `max_per_slot` transfers of the mint may execute in one slot. Every transfer
//! increments a per-mint counter held in a writable account, so the hook mutates state during
//! `Execute` (and the counter must roll back when the enclosing transaction fails).
//!
//! ## Accounts (N = 2 resolved extra accounts)
//!
//! `Execute` receives `[source, mint, destination, owner, validation_list, policy, stats]`.
//! The validation list is generic (seeds-based, identical for every mint):
//!
//! | Extra | PDA | Flags |
//! |---|---|---|
//! | 0 policy | `["arb-policy", mint]` | read-only |
//! | 1 stats | `["arb-stats", mint]` | writable |
//!
//! A swap leg therefore carries `N + 2 = 4` accounts: policy, stats, this program, and the
//! validation list.
//!
//! ## Layouts (little-endian)
//!
//! Policy (45 bytes): `b"ARBPOLCY"`, `bump u8`, `mint [32]`, `max_per_slot u32`.
//! Stats (29 bytes): `b"ARBSTATS"`, `bump u8`, `slot u64`, `count u32`, `total u64`.
//!
//! ## Error codes
//!
//! See [`ArbError`]; codes start at `0x9001`.

mod constants;
mod error;
mod instruction;
mod pda;
mod processor;
mod state;

pub use constants::*;
pub use error::ArbError;
pub use instruction::init_instruction;
pub use pda::{extra_account_metas, policy_address, stats_address, validation_list_address};
pub use processor::process_instruction;
pub use state::{Policy, Stats};

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
