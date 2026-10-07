//! Reading the hooked mint and authorising setup.

use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
use spl_token_2022::{
    extension::{
        transfer_hook::{get_program_id, TransferHook},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::Mint,
};

use crate::error::KitError;

/// What a hook needs to know about its mint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookMint {
    pub hook_program: Option<Pubkey>,
    pub extension_authority: Option<Pubkey>,
    pub mint_authority: Option<Pubkey>,
    pub supply: u64,
    pub decimals: u8,
}

/// The mint must be Token-2022 owned and carry a TransferHook extension.
pub fn read_hook_mint(mint: &AccountInfo) -> Result<HookMint, ProgramError> {
    if mint.owner != &spl_token_2022::id() {
        return Err(KitError::MintNotToken2022.into());
    }
    let data = mint.try_borrow_data()?;
    let state = StateWithExtensions::<Mint>::unpack(&data)?;
    let extension = state
        .get_extension::<TransferHook>()
        .map_err(|_| KitError::MintHookExtensionMissing)?;
    Ok(HookMint {
        hook_program: get_program_id(&state),
        extension_authority: Option::<Pubkey>::from(extension.authority),
        mint_authority: Option::<Pubkey>::from(state.base.mint_authority),
        supply: state.base.supply,
        decimals: state.base.decimals,
    })
}

/// The mint must point at `program_id`.
pub fn require_hook_program(mint: &HookMint, program_id: &Pubkey) -> Result<(), ProgramError> {
    if mint.hook_program != Some(*program_id) {
        return Err(KitError::MintHookMismatch.into());
    }
    Ok(())
}

/// `authority` must sign and be the mint's live TransferHook authority.
pub fn require_extension_authority(
    mint: &HookMint,
    authority: &AccountInfo,
) -> Result<(), ProgramError> {
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let expected = mint
        .extension_authority
        .ok_or(KitError::AuthorityUnavailable)?;
    if expected != *authority.key {
        return Err(KitError::AuthorityMismatch.into());
    }
    Ok(())
}

/// For rules that assume a fixed supply: nobody may mint (mint and burn are not transfers, so
/// the hook never sees them).
pub fn require_mint_authority_revoked(mint: &HookMint) -> Result<(), ProgramError> {
    if mint.mint_authority.is_some() {
        return Err(KitError::MintAuthorityNotRevoked.into());
    }
    Ok(())
}
