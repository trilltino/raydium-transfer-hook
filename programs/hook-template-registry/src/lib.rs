//! # Hook template registry
//!
//! Optional, permissionless metadata for Transfer Hook templates: who published a descriptor
//! (hashes and keys, no prose) for which hook program and template.
//!
//! **A descriptor is never permission.** There is no allowlist here: nothing in Raydium, in the
//! SDK, or in this repository requires a hook to be described, and the existence of a descriptor
//! says nothing about whether a hook is allowed, safe, tested or audited. Anyone may publish a
//! descriptor for any program, including one they did not write; what a reader may conclude from
//! a descriptor is decided by the reader, not by this program.
//!
//! | Module | Role |
//! |---|---|
//! | [`descriptor`] | the descriptor layout and its address |
//! | [`instruction`] | `Publish`, `Update`, `Close` |
//! | [`error`] | error codes from `0xF001` |

pub mod descriptor;
pub mod error;
pub mod instruction;
mod processor;

pub use processor::process_instruction;

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
