//! Unit tests for the pure parts of the hook: encodings, layouts and error codes.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use spl_tlv_account_resolution::state::ExtraAccountMetaList as List;
use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};

use crate::{pda::validate_list_layout, *};

#[test]
fn execute_instruction_data_matches_spl_interface() {
    let data = execute_instruction_data(0x0102_0304_0506_0708);
    assert_eq!(data[..8], EXECUTE_DISCRIMINATOR);
    assert_eq!(&data[8..], &0x0102_0304_0506_0708u64.to_le_bytes());
    assert_eq!(
        TransferHookInstruction::Execute { amount: 7 }.pack()[..8],
        EXECUTE_DISCRIMINATOR
    );
}

#[test]
fn spl_list_discriminators_match_interface() {
    let init = TransferHookInstruction::InitializeExtraAccountMetaList {
        extra_account_metas: vec![],
    }
    .pack();
    assert_eq!(init[..8], SPL_INITIALIZE_LIST_DISCRIMINATOR);
    let update = TransferHookInstruction::UpdateExtraAccountMetaList {
        extra_account_metas: vec![],
    }
    .pack();
    assert_eq!(update[..8], SPL_UPDATE_LIST_DISCRIMINATOR);
}

#[test]
fn error_codes_are_contiguous_from_0x7001_and_round_trip() {
    for (index, error) in HookError::ALL.iter().enumerate() {
        assert_eq!(error.code(), 0x7001 + index as u32);
        assert_eq!(HookError::from_code(error.code()), Some(*error));
        assert_eq!(
            ProgramError::from(*error),
            ProgramError::Custom(error.code())
        );
    }
    assert_eq!(HookError::TransferExceedsLimit.code(), 0x700b);
    assert_eq!(HookError::from_code(1), None);
    // The code after the last variant is unassigned.
    let next = 0x7001 + u32::try_from(HookError::ALL.len()).unwrap();
    assert_eq!(HookError::from_code(next), None);
}

#[test]
fn canonical_list_constant_matches_what_the_spl_crate_writes() {
    assert_eq!(List::size_of(1).unwrap(), VALIDATION_LIST_LEN);
    let mut data = vec![0u8; VALIDATION_LIST_LEN];
    List::init::<ExecuteInstruction>(&mut data, &[config_extra_account_meta().unwrap()]).unwrap();
    assert_eq!(data, CANONICAL_VALIDATION_LIST);
    validate_list_layout(&data).unwrap();
    // Corrupt variants (including a single flipped bit anywhere) are rejected without panicking.
    for mutate in [
        |d: &mut Vec<u8>| d.truncate(10),
        |d: &mut Vec<u8>| d.clear(),
        |d: &mut Vec<u8>| d.push(0),
        |d: &mut Vec<u8>| d[0] ^= 1,
        |d: &mut Vec<u8>| d[8] ^= 1,
        |d: &mut Vec<u8>| d[12] = 2,
    ] {
        let mut corrupt = data.clone();
        mutate(&mut corrupt);
        assert_eq!(
            validate_list_layout(&corrupt),
            Err(HookError::InvalidValidationList)
        );
    }
    for index in 0..data.len() {
        let mut corrupt = data.clone();
        corrupt[index] ^= 1;
        assert_eq!(
            validate_list_layout(&corrupt),
            Err(HookError::InvalidValidationList),
            "flipped byte {index}"
        );
    }
}

const SAMPLE_PARAMS: [u8; 8] = 1_000u64.to_le_bytes();

fn sample_config() -> HookConfig<'static> {
    HookConfig::new(254, 253, Pubkey::new_unique(), &SAMPLE_PARAMS).unwrap()
}

#[test]
fn config_round_trips_and_is_53_bytes() {
    let config = sample_config();
    let bytes = config.encode();
    assert_eq!(bytes.len(), CONFIG_HEADER_LEN + 8);
    assert_eq!(bytes.len(), 53);
    assert_eq!(HookConfig::decode(&bytes).unwrap(), config);
    assert_eq!(config.max_transfer_limit().unwrap(), 1_000);
}

#[test]
fn config_decode_is_strict() {
    let bytes = sample_config().encode();
    let err = |data: &[u8]| HookConfig::decode(data).unwrap_err();
    assert_eq!(err(&bytes[..20]), HookError::InvalidConfigData);
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert_eq!(err(&bad), HookError::InvalidConfigData);
    let mut bad = bytes.clone();
    bad[8] = 2;
    assert_eq!(err(&bad), HookError::UnsupportedVersion);
    let mut bad = bytes.clone();
    bad.push(0);
    assert_eq!(err(&bad), HookError::InvalidConfigData);
    let mut bad = bytes.clone();
    bad[11] = 0xff;
    bad[12] = 0xff;
    assert_eq!(err(&bad), HookError::ParamsTooLarge);
}

#[test]
fn config_address_uses_the_stored_bump() {
    let program_id = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (address, bump) = config_address(&mint, &program_id);
    let mut config = sample_config();
    config.mint = mint;
    config.bump = bump;
    config.verify_address(&program_id, &mint, &address).unwrap();
    config.bump = bump.wrapping_sub(1);
    assert_eq!(
        config.verify_address(&program_id, &mint, &address),
        Err(HookError::InvalidConfigPda)
    );
    assert_eq!(
        config.verify_address(&program_id, &Pubkey::new_unique(), &address),
        Err(HookError::InvalidConfigPda)
    );
}

#[test]
fn initialize_hook_args_round_trip() {
    let args = InitializeHookArgs::max_transfer(77);
    assert_eq!(InitializeHookArgs::unpack(&args.pack()).unwrap(), args);
    assert!(InitializeHookArgs::unpack(&args.pack()[..5]).is_err());
}

#[test]
fn oversized_params_pack_to_a_length_the_program_rejects_instead_of_wrapping() {
    // 65_536 bytes would wrap to a length of 0 with a plain `as u16` cast.
    let args = InitializeHookArgs {
        params: vec![0; usize::from(u16::MAX) + 1],
    };
    assert_eq!(
        InitializeHookArgs::unpack(&args.pack()),
        Err(ProgramError::Custom(HookError::ParamsTooLarge.code()))
    );
}

#[test]
fn config_params_are_bounded_and_decoded_without_copying() {
    let mint = Pubkey::new_unique();
    let max = [7u8; MAX_PARAMS_LEN];
    let config = HookConfig::new(1, 2, mint, &max).unwrap();
    let bytes = config.encode();
    assert_eq!(bytes.len(), CONFIG_HEADER_LEN + MAX_PARAMS_LEN);
    let decoded = HookConfig::decode(&bytes).unwrap();
    assert_eq!(decoded, config);
    // The decoded params are a view into the account data, not a copy.
    assert_eq!(
        decoded.params().as_ptr(),
        bytes[CONFIG_HEADER_LEN..].as_ptr()
    );
    assert_eq!(
        HookConfig::new(1, 2, mint, &[0u8; MAX_PARAMS_LEN + 1]),
        Err(HookError::ParamsTooLarge)
    );
}
