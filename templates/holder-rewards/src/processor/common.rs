//! Checks and helpers the instructions share.

use solana_program::{
    account_info::AccountInfo, clock::Clock, program_error::ProgramError, pubkey::Pubkey,
    sysvar::Sysvar,
};
use spl_token_2022::{
    extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions},
    state::{Account as TokenAccount, Mint},
};

use crate::{
    error::HolderRewardsError,
    state::{Global, Record},
};

pub(super) fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

/// A global account of this program, whichever mint it belongs to (for instructions that are
/// handed the global and learn the mint from it).
pub(super) fn load_any_global(
    program_id: &Pubkey,
    account: &AccountInfo,
) -> Result<Global, ProgramError> {
    if account.owner != program_id {
        return Err(HolderRewardsError::InvalidGlobal.into());
    }
    Ok(Global::decode(&account.try_borrow_data()?)?)
}

/// The global account of `mint`: owned by this program and recording this mint.
///
/// No address is derived. `Initialize` is the only code that writes a global, and it does so at
/// the one PDA of the mint it records; anyone else can only make accounts that hold zeroed data,
/// which never decode. So ownership, the discriminator and the recorded mint identify it.
pub(super) fn load_global(
    program_id: &Pubkey,
    mint: &Pubkey,
    account: &AccountInfo,
) -> Result<Global, ProgramError> {
    let global = load_any_global(program_id, account)?;
    if global.mint != *mint {
        return Err(HolderRewardsError::InvalidGlobal.into());
    }
    Ok(global)
}

/// A record account that exists: owned by this program and recording `token_account` of `mint`
/// (see [`load_global`] for why no address is derived).
pub(super) fn load_record(
    program_id: &Pubkey,
    token_account: &Pubkey,
    mint: &Pubkey,
    account: &AccountInfo,
) -> Result<Record, ProgramError> {
    if account.owner != program_id {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    let record = Record::decode(&account.try_borrow_data()?)?;
    if record.token_account != *token_account || record.mint != *mint {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    Ok(record)
}

/// The record of `token_account` if it has registered, `None` if its record address still holds
/// nothing (system owned). Anything else at that address is an error.
pub(super) fn load_optional_record(
    program_id: &Pubkey,
    token_account: &Pubkey,
    mint: &Pubkey,
    account: &AccountInfo,
) -> Result<Option<Record>, ProgramError> {
    if account.owner == &solana_program::system_program::id() {
        return Ok(None);
    }
    load_record(program_id, token_account, mint, account).map(Some)
}

/// The reward mint's token program must be one of the two SPL token programs, and own the mint.
pub(super) fn require_token_program(
    token_program: &AccountInfo,
    mint: &AccountInfo,
) -> Result<(), ProgramError> {
    let known = token_program.key == &spl_token_2022::id() || token_program.key == &spl_token::id();
    if !known || mint.owner != token_program.key {
        return Err(HolderRewardsError::RewardAccountMismatch.into());
    }
    Ok(())
}

/// The decimals of a reward mint of either token program.
pub(super) fn mint_decimals(mint: &AccountInfo) -> Result<u8, ProgramError> {
    let data = mint.try_borrow_data()?;
    Ok(StateWithExtensions::<Mint>::unpack(&data)?.base.decimals)
}

/// The extensions of a mint of either token program (none for a classic SPL token).
pub(super) fn mint_extensions(mint: &AccountInfo) -> Result<Vec<ExtensionType>, ProgramError> {
    let data = mint.try_borrow_data()?;
    StateWithExtensions::<Mint>::unpack(&data)?.get_extension_types()
}

/// Extensions that break the balance accounting of the **hooked** mint: a transfer fee credits the
/// destination less than the transferred `amount`, and confidential balances move without a
/// visible transfer.
pub(super) fn is_forbidden_on_hooked_mint(extension: ExtensionType) -> bool {
    matches!(
        extension,
        ExtensionType::TransferFeeConfig
            | ExtensionType::ConfidentialTransferMint
            | ExtensionType::ConfidentialTransferFeeConfig
            | ExtensionType::ConfidentialMintBurn
    )
}

/// The only extensions a **reward** mint may carry: descriptive ones. Anything that changes what
/// a transfer delivers (a fee), who can move the vault's tokens (a permanent delegate, a pause,
/// a close authority, a default frozen state) or how they move (a hook, confidential
/// transfers) would let the vault owe more than it holds or be drained.
pub(super) fn is_allowed_on_reward_mint(extension: ExtensionType) -> bool {
    matches!(
        extension,
        ExtensionType::MetadataPointer
            | ExtensionType::TokenMetadata
            | ExtensionType::GroupPointer
            | ExtensionType::TokenGroup
            | ExtensionType::GroupMemberPointer
            | ExtensionType::TokenGroupMember
    )
}

/// `(mint, owner, amount)` of a token account of either token program.
pub(super) fn token_fields(account: &AccountInfo) -> Result<(Pubkey, Pubkey, u64), ProgramError> {
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    Ok((state.base.mint, state.base.owner, state.base.amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ExtensionType::*;

    #[test]
    fn a_reward_mint_may_only_carry_descriptive_extensions() {
        for allowed in [
            MetadataPointer,
            TokenMetadata,
            GroupPointer,
            TokenGroup,
            GroupMemberPointer,
            TokenGroupMember,
        ] {
            assert!(is_allowed_on_reward_mint(allowed), "{allowed:?}");
        }
        for refused in [
            TransferFeeConfig,
            PermanentDelegate,
            TransferHook,
            Pausable,
            MintCloseAuthority,
            DefaultAccountState,
            NonTransferable,
            ConfidentialTransferMint,
            ConfidentialTransferFeeConfig,
            ConfidentialMintBurn,
            InterestBearingConfig,
            ScaledUiAmount,
        ] {
            assert!(!is_allowed_on_reward_mint(refused), "{refused:?}");
        }
    }

    #[test]
    fn the_hooked_mint_may_not_change_what_a_transfer_delivers_or_hide_balances() {
        for refused in [
            TransferFeeConfig,
            ConfidentialTransferMint,
            ConfidentialTransferFeeConfig,
            ConfidentialMintBurn,
        ] {
            assert!(is_forbidden_on_hooked_mint(refused), "{refused:?}");
        }
        for fine in [
            TransferHook,
            MetadataPointer,
            TokenMetadata,
            PermanentDelegate,
            InterestBearingConfig,
            MintCloseAuthority,
        ] {
            assert!(!is_forbidden_on_hooked_mint(fine), "{fine:?}");
        }
    }
}
