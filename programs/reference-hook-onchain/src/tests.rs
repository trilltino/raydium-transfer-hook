//! Unit tests for the pure parts of the hook: encodings, layouts and error codes.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use spl_tlv_account_resolution::state::ExtraAccountMetaList as List;
use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};

use crate::{processor::validate_list_layout, *};

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
}

#[test]
fn canonical_list_has_the_hand_validated_layout() {
    assert_eq!(List::size_of(1).unwrap(), VALIDATION_LIST_LEN);
    let mut data = vec![0u8; VALIDATION_LIST_LEN];
    List::init::<ExecuteInstruction>(&mut data, &[config_extra_account_meta().unwrap()]).unwrap();
    validate_list_layout(&data).unwrap();
    // Corrupt variants are rejected without panicking.
    for mutate in [
        |d: &mut Vec<u8>| d.truncate(10),
        |d: &mut Vec<u8>| d[0] ^= 1,
        |d: &mut Vec<u8>| d[8] ^= 1,
        |d: &mut Vec<u8>| d[12] = 2,
        |d: &mut Vec<u8>| d.push(0),
    ] {
        let mut corrupt = data.clone();
        mutate(&mut corrupt);
        assert!(validate_list_layout(&corrupt).is_err());
    }
}

fn sample_config() -> HookConfig {
    HookConfig::new(
        254,
        253,
        AuthorityMode::Explicit,
        TEMPLATE_MAX_TRANSFER_V1,
        1,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        0,
        &max_transfer_params(1_000),
    )
    .unwrap()
}

#[test]
fn config_round_trips_and_is_264_bytes() {
    let config = sample_config();
    let bytes = config.encode();
    assert_eq!(bytes.len(), 264);
    assert_eq!(HookConfig::decode(&bytes).unwrap(), config);
    assert_eq!(config.max_transfer_limit().unwrap(), 1_000);
}

#[test]
fn config_decode_is_strict() {
    let bytes = sample_config().encode();
    let err = |data: &[u8]| HookConfig::decode(data).unwrap_err();
    assert_eq!(err(&bytes[..100]), HookError::InvalidConfigData);
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
    bad[200] = 1;
    assert_eq!(err(&bad), HookError::InvalidConfigData);
    let mut bad = bytes.clone();
    bad[11] = 4;
    assert_eq!(err(&bad), HookError::UnsupportedMode);
    let mut bad = bytes.clone();
    bad[48] = 1;
    assert_eq!(err(&bad), HookError::InvalidConfigData);
    let mut bad = bytes.clone();
    bad[CONFIG_HEADER_LEN] ^= 1;
    assert_eq!(err(&bad), HookError::HashMismatch);
    let mut bad = bytes.clone();
    bad[192] = 0xff;
    bad[193] = 0xff;
    assert_eq!(err(&bad), HookError::ParamsTooLarge);
    // Non-explicit modes must not carry a config authority.
    let mut bad = bytes.clone();
    bad[11] = 0;
    assert_eq!(err(&bad), HookError::InvalidConfigData);
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
    let args = InitializeHookArgs::max_transfer(AuthorityMode::Explicit, 77, Pubkey::new_unique());
    assert_eq!(InitializeHookArgs::unpack(&args.pack()).unwrap(), args);
    assert!(InitializeHookArgs::unpack(&args.pack()[..20]).is_err());
}
