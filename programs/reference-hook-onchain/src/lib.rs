#![deny(unsafe_code)]

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, instruction::AccountMeta,
    program::invoke_signed, program_error::ProgramError, pubkey::Pubkey, rent::Rent,
    system_instruction, sysvar::Sysvar,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, state::ExtraAccountMetaList};
use spl_token_2022::{
    extension::{
        transfer_hook::{get_program_id, TransferHook, TransferHookAccount},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::{Account as TokenAccount, Mint},
};
use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};

pub const CONFIG_SEED: &[u8] = b"policy";
pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"THPOLICY";
pub const INITIALIZE_DISCRIMINATOR: [u8; 8] = *b"THINIT01";
pub const INITIALIZE_VALIDATION_DISCRIMINATOR: [u8; 8] = *b"THLIST01";
pub const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];
pub const CONFIG_LEN: usize = 8 + 32 + 8;
entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.starts_with(&INITIALIZE_DISCRIMINATOR) {
        initialize_config(program_id, accounts, instruction_data)
    } else if instruction_data == INITIALIZE_VALIDATION_DISCRIMINATOR {
        initialize_validation_list(program_id, accounts)
    } else {
        process_hook(program_id, accounts, instruction_data)
    }
}

fn initialize_config(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() != 16 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let [config, mint, authority, payer, system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer || !payer.is_signer || !payer.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if *system_program.key != solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (hook_program, hook_authority) = {
        let data = mint.try_borrow_data()?;
        let state = StateWithExtensions::<Mint>::unpack(&data)?;
        let extension = state.get_extension::<TransferHook>()?;
        (
            get_program_id(&state),
            Option::<Pubkey>::from(extension.authority),
        )
    };
    if hook_program != Some(*program_id) || hook_authority != Some(*authority.key) {
        return Err(ProgramError::InvalidAccountData);
    }
    let limit = u64::from_le_bytes(
        instruction_data[8..16]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if limit == 0 {
        return Err(ProgramError::InvalidArgument);
    }
    let (expected_config, bump) =
        Pubkey::find_program_address(&[CONFIG_SEED, mint.key.as_ref()], program_id);
    if config.key != &expected_config || config.owner != &solana_program::system_program::id() {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump_seed = [bump];
    let signer_seeds: &[&[u8]] = &[CONFIG_SEED, mint.key.as_ref(), &bump_seed];
    let lamports = Rent::get()?.minimum_balance(CONFIG_LEN);
    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            config.key,
            lamports,
            CONFIG_LEN as u64,
            program_id,
        ),
        &[payer.clone(), config.clone(), system_program.clone()],
        &[signer_seeds],
    )?;

    let mut data = config.try_borrow_mut_data()?;
    data[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
    data[8..40].copy_from_slice(mint.key.as_ref());
    data[40..48].copy_from_slice(&limit.to_le_bytes());
    Ok(())
}

fn initialize_validation_list(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let [validation_list, mint, config, authority, payer, system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer || !payer.is_signer || !payer.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if *system_program.key != solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (hook_program, hook_authority) = {
        let data = mint.try_borrow_data()?;
        let state = StateWithExtensions::<Mint>::unpack(&data)?;
        let extension = state.get_extension::<TransferHook>()?;
        (
            get_program_id(&state),
            Option::<Pubkey>::from(extension.authority),
        )
    };
    if hook_program != Some(*program_id) || hook_authority != Some(*authority.key) {
        return Err(ProgramError::InvalidAccountData);
    }
    let (expected_config, _) = config_address(mint.key, program_id);
    if config.key != &expected_config || config.owner != program_id {
        return Err(ProgramError::InvalidSeeds);
    }
    let config_data = config.try_borrow_data()?;
    if config_data.len() != CONFIG_LEN
        || config_data[..8] != CONFIG_DISCRIMINATOR
        || config_data[8..40] != mint.key.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let expected_list =
        spl_transfer_hook_interface::get_extra_account_metas_address(mint.key, program_id);
    if validation_list.key != &expected_list
        || validation_list.owner != &solana_program::system_program::id()
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let (config_key, _) = config_address(mint.key, program_id);
    let policy_meta = ExtraAccountMeta::new_with_pubkey(&config_key, false, false)
        .map_err(|_| ProgramError::InvalidArgument)?;
    let account_size = ExtraAccountMetaList::size_of(1)?;
    let (validation_list_pda, bump) =
        spl_transfer_hook_interface::get_extra_account_metas_address_and_bump_seed(
            mint.key, program_id,
        );
    let bump_seed = [bump];
    let signer_seeds: &[&[u8]] = &[b"extra-account-metas", mint.key.as_ref(), &bump_seed];
    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            &validation_list_pda,
            Rent::get()?.minimum_balance(account_size),
            account_size as u64,
            program_id,
        ),
        &[
            payer.clone(),
            validation_list.clone(),
            system_program.clone(),
        ],
        &[signer_seeds],
    )?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut validation_list.try_borrow_mut_data()?,
        &[policy_meta],
    )
}

fn process_hook(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let TransferHookInstruction::Execute { amount } =
        TransferHookInstruction::unpack(instruction_data)?
    else {
        return Err(ProgramError::InvalidInstructionData);
    };
    if instruction_data.len() != 16 || instruction_data[..8] != EXECUTE_DISCRIMINATOR {
        return Err(ProgramError::InvalidInstructionData);
    }
    if accounts.len() < 6 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    solana_program::msg!("Transfer Hook Execute amount={amount}");
    let source = &accounts[0];
    let mint = &accounts[1];
    let destination = &accounts[2];
    let validation_list = &accounts[4];
    let config = &accounts[5];
    if source.owner != &spl_token_2022::id()
        || destination.owner != &spl_token_2022::id()
        || mint.owner != &spl_token_2022::id()
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mint_data = mint.try_borrow_data()?;
    let mint_state = StateWithExtensions::<Mint>::unpack(&mint_data)?;
    if get_program_id(&mint_state) != Some(*program_id) {
        return Err(ProgramError::InvalidAccountData);
    }

    let source_data = source.try_borrow_data()?;
    let source_state = StateWithExtensions::<TokenAccount>::unpack(&source_data)?;
    if source_state.base.mint != *mint.key {
        return Err(ProgramError::InvalidAccountData);
    }
    if !bool::from(
        source_state
            .get_extension::<TransferHookAccount>()?
            .transferring,
    ) {
        return Err(ProgramError::InvalidAccountData);
    }
    drop(source_data);
    let destination_data = destination.try_borrow_data()?;
    let destination_state = StateWithExtensions::<TokenAccount>::unpack(&destination_data)?;
    if destination_state.base.mint != *mint.key {
        return Err(ProgramError::InvalidAccountData);
    }
    if !bool::from(
        destination_state
            .get_extension::<TransferHookAccount>()?
            .transferring,
    ) {
        return Err(ProgramError::InvalidAccountData);
    }
    drop(destination_data);

    let expected_list =
        spl_transfer_hook_interface::get_extra_account_metas_address(mint.key, program_id);
    if validation_list.key != &expected_list || validation_list.owner != program_id {
        return Err(ProgramError::InvalidAccountData);
    }
    let validation_data = validation_list.try_borrow_data()?;
    ExtraAccountMetaList::check_account_infos::<ExecuteInstruction>(
        accounts,
        instruction_data,
        program_id,
        &validation_data,
    )?;

    let (expected_config, _) =
        Pubkey::find_program_address(&[CONFIG_SEED, mint.key.as_ref()], program_id);
    if config.key != &expected_config || config.owner != program_id {
        return Err(ProgramError::InvalidSeeds);
    }
    let config_data = config.try_borrow_data()?;
    if config_data.len() != CONFIG_LEN
        || config_data[..8] != CONFIG_DISCRIMINATOR
        || config_data[8..40] != mint.key.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let limit = u64::from_le_bytes(
        config_data[40..48]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    if amount > limit {
        return Err(ProgramError::Custom(1));
    }
    Ok(())
}

pub fn initialize_config_instruction(
    program_id: Pubkey,
    config: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    payer: Pubkey,
    limit: u64,
) -> solana_program::instruction::Instruction {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&INITIALIZE_DISCRIMINATOR);
    data.extend_from_slice(&limit.to_le_bytes());
    solana_program::instruction::Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_program::system_program::id(), false),
        ],
        data,
    }
}

pub fn initialize_validation_list_instruction(
    program_id: Pubkey,
    validation_list: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    payer: Pubkey,
) -> solana_program::instruction::Instruction {
    solana_program::instruction::Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(validation_list, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(config_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_program::system_program::id(), false),
        ],
        data: INITIALIZE_VALIDATION_DISCRIMINATOR.to_vec(),
    }
}

pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED, mint.as_ref()], program_id)
}

pub fn execute_instruction_data(amount: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&EXECUTE_DISCRIMINATOR);
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_instruction_data_matches_spl_interface() {
        let data = execute_instruction_data(0x0102_0304_0506_0708);
        assert_eq!(data[..8], EXECUTE_DISCRIMINATOR);
        assert_eq!(&data[8..], &0x0102_0304_0506_0708u64.to_le_bytes());
    }
}
