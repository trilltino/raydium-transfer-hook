//! MODEL ONLY: a policy simulator, not a LaunchLab integration and not a CPI.
//!
//! LaunchLab's handlers are not public, so there is no real LaunchLab ABI to
//! frame against and this crate offers none. What remains is
//! [`LaunchPolicySimulator`]: a small state machine that applies the platform
//! policy model to a launch's lifecycle, and can hand the SDK the
//! [`ResolveOptions`] a trade on that launch would have to satisfy. It is gated
//! behind the `model` feature so it is never mistaken for an API.

#![forbid(unsafe_code)]

#[cfg(feature = "model")]
mod error;
#[cfg(feature = "model")]
mod phase;
#[cfg(feature = "model")]
mod simulator;

#[cfg(feature = "model")]
pub use error::LaunchSimError;
#[cfg(feature = "model")]
pub use phase::{GraduationRecord, LaunchPhase};
#[cfg(feature = "model")]
pub use simulator::LaunchPolicySimulator;
