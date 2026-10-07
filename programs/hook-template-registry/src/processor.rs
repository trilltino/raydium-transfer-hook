//! `Publish`, `Update` and `Close`.

use hook_kit::create_pda;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{
    descriptor::{descriptor_address, Descriptor, DESCRIPTOR_LEN, DESCRIPTOR_VERSION},
    error::RegistryError,
    instruction::RegistryInstruction,
};

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    match RegistryInstruction::unpack(data)? {
        RegistryInstruction::Publish {
            template_id,
            manifest_hash,
            flags,
        } => publish(program_id, accounts, template_id, manifest_hash, flags),
        RegistryInstruction::Update {
            manifest_hash,
            flags,
        } => update(program_id, accounts, manifest_hash, flags),
        RegistryInstruction::Close => close(program_id, accounts),
    }
}

fn publish(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    template_id: [u8; 32],
    manifest_hash: [u8; 32],
    flags: u64,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let publisher = next_account_info(accounts_iter)?;
    let hook_program = next_account_info(accounts_iter)?;
    let descriptor_account = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !publisher.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    // The only fact checked about the hook: that it is a program at all. Nothing says it is a good
    // one, a tested one, or the publisher's own.
    if !hook_program.executable {
        return Err(RegistryError::HookProgramNotExecutable.into());
    }
    let (expected, bump) =
        descriptor_address(program_id, hook_program.key, &template_id, publisher.key);
    if descriptor_account.key != &expected {
        return Err(RegistryError::InvalidDescriptor.into());
    }
    create_pda(
        publisher,
        descriptor_account,
        system_program,
        program_id,
        DESCRIPTOR_LEN,
        &[
            b"hook-template",
            hook_program.key.as_ref(),
            &template_id,
            publisher.key.as_ref(),
            &[bump],
        ],
    )?;
    Descriptor {
        bump,
        version: DESCRIPTOR_VERSION,
        hook_program: *hook_program.key,
        template_id,
        manifest_hash,
        template_authority: *publisher.key,
        flags,
    }
    .encode_into(&mut descriptor_account.try_borrow_mut_data()?)?;
    Ok(())
}

/// The descriptor, if `descriptor_account` is one of ours and `authority` signed for it.
fn load_for_authority(
    program_id: &Pubkey,
    authority: &AccountInfo,
    descriptor_account: &AccountInfo,
) -> Result<Descriptor, ProgramError> {
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if descriptor_account.owner != program_id {
        return Err(RegistryError::InvalidDescriptor.into());
    }
    let descriptor = Descriptor::decode(&descriptor_account.try_borrow_data()?)?;
    if descriptor.template_authority != *authority.key {
        return Err(RegistryError::NotTemplateAuthority.into());
    }
    Ok(descriptor)
}

fn update(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    manifest_hash: [u8; 32],
    flags: u64,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let authority = next_account_info(accounts_iter)?;
    let descriptor_account = next_account_info(accounts_iter)?;
    let descriptor = load_for_authority(program_id, authority, descriptor_account)?;
    Descriptor {
        manifest_hash,
        flags,
        ..descriptor
    }
    .encode_into(&mut descriptor_account.try_borrow_mut_data()?)?;
    Ok(())
}

fn close(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let authority = next_account_info(accounts_iter)?;
    let descriptor_account = next_account_info(accounts_iter)?;
    load_for_authority(program_id, authority, descriptor_account)?;
    let lamports = descriptor_account.lamports();
    **authority.try_borrow_mut_lamports()? = authority
        .lamports()
        .checked_add(lamports)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    **descriptor_account.try_borrow_mut_lamports()? = 0;
    descriptor_account.try_borrow_mut_data()?.fill(0);
    descriptor_account.assign(&solana_program::system_program::id());
    descriptor_account.realloc(0, false)?;
    Ok(())
}
