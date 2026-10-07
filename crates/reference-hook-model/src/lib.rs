//! MODEL ONLY: a pure-Rust model of hook policy rules. The deployable program is
//! `reference-hook-onchain`, which enforces only a subset (see its README); the
//! allow/deny lists, authority and timelock behavior here are not on-chain.

#![forbid(unsafe_code)]

mod config;
mod engine;
mod error;

pub use config::{ConfigAuthorization, ConfigError, HookModule, MintHookConfig};
pub use engine::HookEngine;
pub use error::HookError;
