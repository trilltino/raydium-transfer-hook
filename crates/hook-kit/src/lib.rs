#![forbid(unsafe_code)]
//! Plumbing every Token-2022 Transfer Hook needs, so a hook is mostly its rule.
//!
//! | Module | Role |
//! |---|---|
//! | [`execute`] | [`execute_prelude`]: every check a hook must make before its own rule |
//! | [`mint`] | reading the hooked mint, authorising setup |
//! | [`token`] | reading a token account (post-transfer balance, owner, the `transferring` flag) |
//! | [`accounts`] | creating PDAs (surviving a pre-funded address) and the canonical validation list |
//! | [`error`] | [`KitError`], codes from `0x8001` |
//!
//! Two facts a rule depends on, both verified against the Token-2022 source:
//!
//! * Token-2022 moves the tokens **before** it calls the hook, so `Execute` sees post-transfer
//!   balances.
//! * `Execute` is built with every account read-only and the owner slot non-signer, so a hook
//!   cannot spend the transfer authority. It can sign for its own PDAs.
//!
//! With the `test-support` feature, [`testing`] builds a hooked Token-2022 mint and funded
//! accounts in-process and sends transfers that resolve the hook's accounts through the SDK.

/// How many PDA-derived extra accounts a hook can practically declare today. Measured by
/// `benches/` (a hook that does nothing but declare N extras): the hook program's 32 KiB heap runs out
/// at 12 to 14 extras and Token-2022's at 16 and above, before transaction size or compute matter.
/// Asking for a larger heap frame did not help in those runs. Literal-address extras are cheaper
/// and were not measured, so treat this as a ceiling to stay well under, not a target.
/// See `docs/commercial-and-limits.md`.
pub const PRACTICAL_EXTRA_ACCOUNTS: usize = 10;

pub mod accounts;
pub mod error;
pub mod execute;
pub mod mint;
pub mod token;

#[cfg(feature = "test-support")]
pub mod testing;

pub use accounts::{create_pda, create_validation_list, validation_list_address};
pub use error::KitError;
pub use execute::{execute_prelude, parse_execute_amount, ExecuteCtx, EXECUTE_DISCRIMINATOR};
pub use mint::{
    read_hook_mint, require_extension_authority, require_hook_program,
    require_mint_authority_revoked, HookMint,
};
pub use token::{read_token_account, TokenView};
