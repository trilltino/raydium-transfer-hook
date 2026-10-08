//! The caller-facing account and transfer types.

use solana_program::pubkey::Pubkey;

/// An account as returned by the caller's fetcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplAccount {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// One token transfer inside a larger instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplTransferLeg {
    pub source: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
    pub amount: u64,
}
