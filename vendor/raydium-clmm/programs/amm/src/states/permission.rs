use anchor_lang::prelude::*;

/// Seed prefix for the permission PDA: `[PERMISSION_SEED, authority]`.
pub const PERMISSION_SEED: &str = "permission";

/// Grants its `authority` the right to call permissioned instructions
/// (e.g. `create_permissioned_pool`). Existence of the PDA is the grant.
#[account]
#[derive(Default, Debug)]
pub struct Permission {
    /// The account authorized by this permission PDA.
    pub authority: Pubkey,
    /// padding
    pub padding: [u64; 30],
}

impl Permission {
    pub const LEN: usize = 8 + 32 + 8 * 30;
}
