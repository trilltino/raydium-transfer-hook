use crate::error::ErrorCode;
use crate::states::*;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ClosePermissionPda<'info> {
    #[account(
        mut,
        constraint = (owner.key() == crate::admin::ID || owner.key() == crate::permission_pda_admin::ID) @ ErrorCode::NotApproved
    )]
    pub owner: Signer<'info>,

    /// CHECK: permission account authority
    pub permission_authority: UncheckedAccount<'info>,

    /// The permission account to close; rent is refunded to `owner`.
    #[account(
        mut,
        seeds = [
            PERMISSION_SEED.as_bytes(),
            permission_authority.key().as_ref()
        ],
        bump,
        close = owner
    )]
    pub permission: Account<'info, Permission>,
}

pub fn close_permission_pda(_ctx: Context<ClosePermissionPda>) -> Result<()> {
    Ok(())
}
