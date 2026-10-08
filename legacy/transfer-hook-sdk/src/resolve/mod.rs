//! Atomic, attributed resolution of Token-2022 Transfer Hook accounts.
//!
//! The only resolver in this crate is built on the official
//! `spl-transfer-hook-interface` offchain helper and `spl-tlv-account-resolution`
//! lists. Resolution is pure: it works on a private scratch instruction and
//! returns a [`LegHook`]; nothing is appended to a caller's instruction until a
//! `frame_*` function does so after validating every leg.

mod accounts;
mod fingerprint;
mod inspect;
mod leg;
mod options;
mod privilege;
mod slice;

pub use accounts::{SplAccount, SplTransferLeg};
pub use fingerprint::{HookFingerprint, ProgramFingerprint};
pub use leg::{resolve_leg, resolve_legs};
pub use options::{default_allowed_loaders, ResolveOptions, LOADER_V4_ID, RAYDIUM_PROGRAM_IDS};
pub use privilege::PrivilegePolicy;
pub use slice::{HookSlice, LegHook};

#[cfg(test)]
pub(crate) use inspect::{execute_discriminator, invalid_hook_program_reason};
