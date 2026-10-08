//! What the plumbing hands to the rule for one transfer.

use solana_program::pubkey::Pubkey;

/// One Token-2022 transfer, as the hook sees it. Built by `process_execute` after every
/// plumbing check has passed, so a rule can trust it.
///
/// Token-2022 moves the tokens before it calls the hook, so token-account balances read inside a
/// hook are post-transfer balances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferContext {
    /// The transferred amount, in raw token units.
    pub amount: u64,
    /// The token account the tokens left.
    pub source: Pubkey,
    /// The token account the tokens arrived in.
    pub destination: Pubkey,
    /// The hooked mint.
    pub mint: Pubkey,
    /// The source account's owner or delegate that authorised the transfer.
    pub authority: Pubkey,
}
