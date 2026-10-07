#![deny(unsafe_code)]
//! Reference Token-2022 Transfer Hook program (max-transfer rule only).
//!
//! # What this program enforces
//!
//! A single rule template, `max-transfer-v1`: a hooked transfer of `amount > limit` is rejected
//! with [`HookError::TransferExceedsLimit`]. Nothing else is enforced. Allow/deny lists,
//! timelocks and platform-level policy exist only as models in `crates/reference-hook-model`.
//!
//! # Module map
//!
//! | Module | Role |
//! |---|---|
//! | [`rule`] | the commercial decision; the one file a hook author normally edits |
//! | `constants` | seeds, discriminators, sizes |
//! | `error` | [`HookError`] and its codes |
//! | `authority` | [`AuthorityMode`] |
//! | `config` | the [`HookConfig`] byte layout |
//! | `pda` | addresses and the validation list's extra account |
//! | `instruction` | client-side instruction builders |
//! | `processor` | on-chain handlers: `initialize`, `mutate`, `execute`, shared `common` |
//!
//! # Accounts per mint
//!
//! * Config PDA, seeds `["hook-config", mint]`, owned by this program, `256 + params_len` bytes.
//! * SPL validation list PDA, seeds `["extra-account-metas", mint]`, holding exactly one
//!   seeds-based `ExtraAccountMeta` (`Literal "hook-config"` + `AccountKey{index: 1}`). The
//!   list bytes are therefore identical for every mint and never need migration.
//!
//! Both are created by one atomic [`HookInstruction::InitializeHook`] so a mint can never end up
//! with a config but no list (or the reverse).
//!
//! # Config byte layout (little-endian, packed, fixed offsets)
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 8 | `b"HKCONFIG"` |
//! | 8 | 1 | version (`1`) |
//! | 9 | 1 | bump of the config PDA |
//! | 10 | 1 | bump of the validation-list PDA |
//! | 11 | 1 | authority mode ([`AuthorityMode`]) |
//! | 12 | 4 | template_version |
//! | 16 | 32 | mint |
//! | 48 | 32 | platform_config (reserved, must be all zero) |
//! | 80 | 32 | config_authority (mode 2 only, otherwise all zero) |
//! | 112 | 32 | template_id (`"max-transfer-v1"` zero padded) |
//! | 144 | 32 | config_hash = `sha256(template_id \|\| template_version_le \|\| params)` |
//! | 176 | 8 | flags (no flag is defined yet, must be zero) |
//! | 184 | 8 | config_seq (incremented by every mutation) |
//! | 192 | 2 | params_len (`<= 256`) |
//! | 194 | 62 | reserved (must be all zero) |
//! | 256 | n | params (`max-transfer-v1`: `u64` limit, `n == 8`, limit > 0) |
//!
//! Parsing is strict: `len == 256 + params_len`, the discriminator, version, reserved bytes and
//! hash must all match. See [`HookConfig::decode`].
//!
//! # Instructions (8-byte ASCII discriminator, then little-endian fields)
//!
//! * `InitializeHook` (`b"HKINIT01"`): `mode u8, template_version u32, flags u64,
//!   template_id [u8;32], config_authority [u8;32], params_len u16, params`.
//!   Accounts: `config (w), validation_list (w), mint, authority (s), payer (s, w), system`.
//! * `UpdateConfig` (`b"HKUPDT01"`): `expected_seq u64, template_version u32, flags u64,
//!   params_len u16, params`. Accounts: `config (w), mint, authority (s)`.
//! * `SetConfigAuthority` (`b"HKSETAU1"`): `new [u8;32]`. Accounts: `config (w), mint, authority (s)`.
//!   Valid only in mode 2. A zero `new` is the one-way transition to mode 3 (Immutable).
//! * `Execute` (SPL interface discriminator): exactly six accounts
//!   `source, mint, destination, owner, validation_list, config`.
//!
//! SPL `InitializeExtraAccountMetaList` / `UpdateExtraAccountMetaList` are NOT aliased. They carry
//! no authority mode, template or params, so any alias would have to invent a rule for the mint
//! (for example "unlimited"), which weakens the checks. They are rejected with
//! [`HookError::SplInterfaceUnsupported`]; use `InitializeHook`.
//!
//! # Authority modes
//!
//! | Mode | Name | Init signer | Update / SetConfigAuthority signer |
//! |---|---|---|---|
//! | 0 | ExtensionAuthority | live `TransferHook.authority` | live `TransferHook.authority` |
//! | 1 | MintAuthority | live mint authority | live mint authority |
//! | 2 | Explicit | live `TransferHook.authority` (consent) | stored `config_authority` |
//! | 3 | Immutable | live `TransferHook.authority` | nobody (`ConfigImmutable`) |
//! | 4+ | reserved (PlatformControlled) | `UnsupportedMode` | `UnsupportedMode` |
//!
//! A required authority that is `None` yields [`HookError::AuthorityUnavailable`]. The BPF
//! upgrade authority of this program is a third, out-of-band authority: it can replace the code
//! and therefore the rule for every mint, whatever the per-mint mode says.
//!
//! # Error codes (`ProgramError::Custom`)
//!
//! See [`HookError`]; codes start at `0x7001`.

mod authority;
mod config;
mod constants;
mod error;
mod instruction;
mod pda;
mod processor;
pub mod rule;
#[cfg(test)]
mod tests;

pub use authority::AuthorityMode;
pub use config::{compute_config_hash, max_transfer_params, HookConfig};
pub use constants::*;
pub use error::HookError;
pub use instruction::{
    initialize_hook_instruction, set_config_authority_instruction, update_config_instruction,
    InitializeHookArgs,
};
pub use pda::{
    config_address, config_extra_account_meta, execute_instruction_data, validation_list_address,
};
pub use processor::process_instruction;

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);
