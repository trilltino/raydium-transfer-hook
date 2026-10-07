//! # Bench hook
//!
//! A hook that exists to be **measured**. The number of extra accounts it declares is a parameter
//! (`Initialize { extras, writable_counter }`), and `Execute` does nothing but the shared checks
//! (and, if asked, one write). So what a benchmark sees as it raises `extras` is the cost of a
//! thicker hook (accounts, transaction bytes, compute), not of a clever rule.
//!
//! * The extras are program-derived addresses of this program, one set per mint. They need not
//!   exist: a read-only account may be referenced without existing.
//! * With `writable_counter` the first extra is a writable counter that every `Execute` increments,
//!   which is what a hook with shared state looks like to the scheduler.
//!
//! It is a test fixture. It has no rule and no authority beyond "the mint's hook authority may
//! initialise it", and must never be pointed at a real token.

mod processor;

pub use processor::{
    counter_address, extra_address, initialize_instruction, process_instruction, MAX_EXTRAS,
};

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
