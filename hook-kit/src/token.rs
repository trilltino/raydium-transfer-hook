//! Reading a Token-2022 token account from inside a hook.

use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
use spl_token_2022::{
    extension::{transfer_hook::TransferHookAccount, BaseStateWithExtensions, StateWithExtensions},
    state::Account as TokenAccount,
};

/// The fields a rule usually needs. During `Execute`, `amount` is the **post-transfer** balance:
/// Token-2022 moves the tokens before it calls the hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenView {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    /// Set by Token-2022 for the duration of a transfer, and only then.
    pub transferring: bool,
}

/// # Errors
/// `IncorrectProgramId` unless the account is a Token-2022 token account, or the unpack error.
pub fn read_token_account(account: &AccountInfo) -> Result<TokenView, ProgramError> {
    if account.owner != &spl_token_2022::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    let transferring = state
        .get_extension::<TransferHookAccount>()
        .map(|extension| bool::from(extension.transferring))
        .unwrap_or(false);
    Ok(TokenView {
        mint: state.base.mint,
        owner: state.base.owner,
        amount: state.base.amount,
        transferring,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{hooked_account, with_account};

    #[test]
    fn the_transferring_flag_and_post_transfer_balance_are_read() {
        let (key, mint, owner) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        for transferring in [false, true] {
            let mut data = hooked_account(mint, owner, 77, transferring);
            let view = with_account(
                &key,
                &spl_token_2022::id(),
                false,
                &mut data,
                read_token_account,
            )
            .unwrap();
            assert_eq!(
                view,
                TokenView {
                    mint,
                    owner,
                    amount: 77,
                    transferring
                }
            );
        }
    }

    #[test]
    fn an_account_of_another_program_or_with_garbage_data_is_refused() {
        let key = Pubkey::new_unique();
        let mut data = hooked_account(Pubkey::new_unique(), Pubkey::new_unique(), 1, true);
        assert_eq!(
            with_account(
                &key,
                &Pubkey::new_unique(),
                false,
                &mut data,
                read_token_account
            ),
            Err(ProgramError::IncorrectProgramId)
        );
        let mut garbage = vec![7u8; 40];
        assert!(with_account(
            &key,
            &spl_token_2022::id(),
            false,
            &mut garbage,
            read_token_account
        )
        .is_err());
    }
}
