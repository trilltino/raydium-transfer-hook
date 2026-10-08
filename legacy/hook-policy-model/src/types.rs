//! Plain value types shared by the model.

/// A public key as raw bytes, so this crate stays free of Solana types.
pub type Pubkey = [u8; 32];

/// The transfer a model hook engine evaluates. (The SDK uses its own
/// `SplTransferLeg` with Solana key types; this is for the pure-Rust model.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferContext {
    pub source: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
    pub amount: u64,
}
