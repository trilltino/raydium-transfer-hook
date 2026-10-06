use crate::error::ErrorCode;
use crate::states::*;
use anchor_lang::prelude::*;
use std::ops::DerefMut;

pub mod permission_pda_admin {
    use super::{pubkey, Pubkey};
    #[cfg(feature = "devnet")]
    pub const ID: Pubkey = pubkey!("DRaybMsRNbqoKazUkpZVJ5A2zodQVfoUqC4Cm8fv1mgD");
    #[cfg(not(feature = "devnet"))]
    pub const ID: Pubkey = pubkey!("RayzVBPm6p6xtG7fU3KX4k44UexK4NjewDk2QCwoLqa");
}

#[derive(Accounts)]
pub struct CreatePermissionPda<'info> {
    #[account(
        mut,
        constraint = (owner.key() == crate::admin::ID || owner.key() == crate::permission_pda_admin::ID) @ ErrorCode::NotApproved
    )]
    pub owner: Signer<'info>,

    /// CHECK: permission account authority
    pub permission_authority: UncheckedAccount<'info>,

    /// The permission account to create, granting `permission_authority` the
    /// right to call permissioned instructions.
    #[account(
        init,
        seeds = [
            PERMISSION_SEED.as_bytes(),
            permission_authority.key().as_ref()
        ],
        bump,
        payer = owner,
        space = Permission::LEN
    )]
    pub permission: Account<'info, Permission>,

    pub system_program: Program<'info, System>,
}

pub fn create_permission_pda(ctx: Context<CreatePermissionPda>) -> Result<()> {
    let permission = ctx.accounts.permission.deref_mut();
    permission.authority = ctx.accounts.permission_authority.key();
    Ok(())
}
