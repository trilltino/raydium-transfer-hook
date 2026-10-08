#![deny(unsafe_code)]
//! Transfer Hook starter: a Token-2022 Transfer Hook with the plumbing done (default rule: max transfer).
//!
//! # What this program enforces
//!
//! A hooked transfer of `amount > limit` is rejected with [`HookError::TransferExceedsLimit`].
//! Change the rule in `rule.rs`.
//!
//! # Module map
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the business decision; the one file a hook author normally edits |
//! | `constants` | seeds, discriminators, sizes |
//! | `error` | [`HookError`] and its codes |
//! | `config` | the [`HookConfig`] byte layout |
//! | `context` | the [`TransferContext`] handed to the rule |
//! | `pda` | addresses and the validation list's extra account |
//! | `instruction` | client-side instruction builder |
//! | `processor` | on-chain handlers: `initialize`, `execute`, shared `common` |
//!
//! # Accounts per mint
//!
//! * Config PDA, seeds `["hook-config", mint]`, owned by this program, `45 + params_len` bytes.
//! * SPL validation list PDA, seeds `["extra-account-metas", mint]`, holding exactly one
//!   seeds-based `ExtraAccountMeta` (`Literal "hook-config"` + `AccountKey{index: 1}`). The
//!   list bytes are therefore identical for every mint and never need migration.
//!
//! Both are created by one atomic `InitializeHook` so a mint can never end up with a config but
//! no list (or the reverse).
//!
//! # Config byte layout (little-endian, packed, fixed offsets)
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 8 | `b"HKCONFIG"` |
//! | 8 | 1 | version (`1`) |
//! | 9 | 1 | bump of the config PDA |
//! | 10 | 1 | bump of the validation-list PDA |
//! | 11 | 2 | params_len (`<= 256`) |
//! | 13 | 32 | mint |
//! | 45 | n | params (default rule: `u64` limit, `n == 8`, limit > 0) |
//!
//! Parsing is strict: `len == 45 + params_len`, the discriminator and version must match. See
//! [`HookConfig::decode`].
//!
//! # Instructions (8-byte ASCII discriminator, then little-endian fields)
//!
//! * `InitializeHook` (`b"HKINIT01"`): `params_len u16, params`.
//!   Accounts: `config (w), validation_list (w), mint, authority (s), payer (s, w), system`.
//!   `authority` must be the mint's live Transfer Hook extension authority. It runs once per
//!   mint: the config cannot be changed afterwards.
//! * `Execute` (SPL interface discriminator): exactly six accounts
//!   `source, mint, destination, owner, validation_list, config`.
//!
//! SPL `InitializeExtraAccountMetaList` / `UpdateExtraAccountMetaList` are NOT aliased. They carry
//! no params, so any alias would have to invent a rule for the mint (for example "unlimited"),
//! which weakens the checks. They are rejected with [`HookError::SplInterfaceUnsupported`]; use
//! `InitializeHook`.
//!
//! # Who holds which power
//!
//! * The mint's Transfer Hook extension authority initialises the hook (once). It can also re-point
//!   the mint at a different hook program, which bypasses this one entirely.
//! * The BPF upgrade authority of this program can replace the code, and therefore the rule, for
//!   every mint, whatever the config says.
//!
//! Need a config that can change later, or other authority models? Add an `UpdateConfig`
//! instruction that checks the signer and a sequence number; the git history before the starter was
//! slimmed (tag `pre-community-hook-kit`) has a worked version with four authority modes.
//!
//! # Error codes (`ProgramError::Custom`)
//!
//! See [`HookError`]; codes start at `0x7001`.

mod config;
mod constants;
mod context;
mod error;
mod instruction;
mod pda;
mod processor;
pub mod rule;
#[cfg(test)]
mod tests;

pub use config::{max_transfer_params, HookConfig};
pub use constants::*;
pub use context::TransferContext;
pub use error::HookError;
pub use instruction::{initialize_hook_instruction, InitializeHookArgs};
pub use pda::{
    config_address, config_extra_account_meta, execute_instruction_data, validation_list_address,
};
pub use processor::process_instruction;

// The entrypoint macro tests cfgs (`solana`, `custom-heap`, `custom-panic`) of the crate using it.
#[cfg(not(feature = "no-entrypoint"))]
#[allow(unexpected_cfgs)]
mod entrypoint {
    use super::process_instruction;
    solana_program::entrypoint!(process_instruction);
}
