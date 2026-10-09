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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{hooked_mint, plain_mint, with_account};

    const MINT_LEN_ERROR: u32 = KitError::MintNotToken2022 as u32;

    fn custom(error: KitError) -> ProgramError {
        ProgramError::Custom(error.code())
    }

    #[test]
    fn a_mint_not_owned_by_token_2022_is_refused() {
        let (key, other) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut data = hooked_mint(None, None, None, 0);
        let result = with_account(&key, &other, false, &mut data, read_hook_mint);
        assert_eq!(result, Err(ProgramError::Custom(MINT_LEN_ERROR)));
    }

    #[test]
    fn a_mint_without_the_hook_extension_is_refused() {
        let key = Pubkey::new_unique();
        let mut data = plain_mint();
        let result = with_account(
            &key,
            &spl_token_2022::id(),
            false,
            &mut data,
            read_hook_mint,
        );
        assert_eq!(result, Err(custom(KitError::MintHookExtensionMissing)));
    }

    #[test]
    fn a_hooked_mint_reports_its_hook_authority_and_supply() {
        let (key, program, authority, minter) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let mut data = hooked_mint(Some(authority), Some(program), Some(minter), 42);
        let mint = with_account(
            &key,
            &spl_token_2022::id(),
            false,
            &mut data,
            read_hook_mint,
        )
        .unwrap();
        assert_eq!(
            mint,
            HookMint {
                hook_program: Some(program),
                extension_authority: Some(authority),
                mint_authority: Some(minter),
                supply: 42,
                decimals: 6,
            }
        );
        assert_eq!(require_hook_program(&mint, &program), Ok(()));
        assert_eq!(
            require_hook_program(&mint, &Pubkey::new_unique()),
            Err(custom(KitError::MintHookMismatch))
        );
        assert_eq!(
            require_mint_authority_revoked(&mint),
            Err(custom(KitError::MintAuthorityNotRevoked))
        );
    }

    #[test]
    fn a_revoked_mint_authority_and_hook_program_read_as_none() {
        let key = Pubkey::new_unique();
        let mut data = hooked_mint(None, None, None, 0);
        let mint = with_account(
            &key,
            &spl_token_2022::id(),
            false,
            &mut data,
            read_hook_mint,
        )
        .unwrap();
        assert_eq!(mint.hook_program, None);
        assert_eq!(mint.extension_authority, None);
        assert_eq!(require_mint_authority_revoked(&mint), Ok(()));
        assert_eq!(
            require_hook_program(&mint, &Pubkey::new_unique()),
            Err(custom(KitError::MintHookMismatch))
        );
    }

    #[test]
    fn only_the_signing_live_extension_authority_may_set_up() {
        let (authority, program) = (Pubkey::new_unique(), Pubkey::new_unique());
        let live = HookMint {
            hook_program: Some(program),
            extension_authority: Some(authority),
            mint_authority: None,
            supply: 0,
            decimals: 0,
        };
        let revoked = HookMint {
            extension_authority: None,
            ..live
        };
        let check = |mint: &HookMint, key: &Pubkey, signer: bool| {
            with_account(key, &program, signer, &mut [], |info| {
                require_extension_authority(mint, info)
            })
        };
        assert_eq!(check(&live, &authority, true), Ok(()));
        assert_eq!(
            check(&live, &authority, false),
            Err(ProgramError::MissingRequiredSignature)
        );
        assert_eq!(
            check(&live, &Pubkey::new_unique(), true),
            Err(custom(KitError::AuthorityMismatch))
        );
        assert_eq!(
            check(&revoked, &authority, true),
            Err(custom(KitError::AuthorityUnavailable))
        );
    }
}
