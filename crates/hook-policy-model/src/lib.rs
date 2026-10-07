//! MODEL ONLY: platform policy rules, not an on-chain program or a Raydium CPI.
//!
//! Keys here are plain `[u8; 32]` so this crate stays free of Solana types. The
//! SDK converts a [`PolicyDecision`] into its resolution options.

#![forbid(unsafe_code)]

mod error;
mod platform;
mod policy;
mod types;

pub use error::PolicyError;
pub use platform::{ConfigActor, LaunchConfig, PlatformConfig, PolicyDecision};
pub use policy::{HookAuthorityPolicy, HookPolicy, HookPreset};
pub use types::{Pubkey, TransferContext};
