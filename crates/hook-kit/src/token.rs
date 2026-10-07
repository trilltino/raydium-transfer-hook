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
