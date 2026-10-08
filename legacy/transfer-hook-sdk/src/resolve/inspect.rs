//! Reading and validating the on-chain accounts a resolution depends on.

use std::future::Future;

use solana_program::{bpf_loader_upgradeable, hash::hash, pubkey::Pubkey};
use spl_tlv_account_resolution::account::ExtraAccountMeta;
use spl_token_2022::{
    extension::{transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions},
    state::Mint as Token2022Mint,
};
use spl_transfer_hook_interface::{
    get_extra_account_metas_address, instruction::TransferHookInstruction,
};

use super::{
    accounts::SplAccount,
    fingerprint::{HookFingerprint, ProgramFingerprint},
    options::{ResolveOptions, LOADER_V4_ID, RAYDIUM_PROGRAM_IDS},
};
use crate::error::{AuthorityExpectation, FetchError, HookProgramInvalidReason, SplResolveError};

const UPGRADEABLE_PROGRAM_TAG: u32 = 2;
const UPGRADEABLE_PROGRAM_DATA_TAG: u32 = 3;
const UPGRADEABLE_UNINITIALIZED_TAG: u32 = 0;
const UPGRADEABLE_PROGRAM_LEN: usize = 36;
const UPGRADEABLE_PROGRAM_DATA_META_LEN: usize = 45;
const LOADER_V4_HEADER_LEN: usize = 48;
const LOADER_V4_STATUS_OFFSET: usize = 40;
const TLV_ENTRY_HEADER_LEN: usize = 12;
const POD_SLICE_PREFIX_LEN: usize = 4;

pub(super) struct Inspection {
    pub(super) hook_program: Pubkey,
    pub(super) list_address: Pubkey,
    pub(super) list_data: Vec<u8>,
    pub(super) fingerprint: HookFingerprint,
}

pub(super) async fn fetch_checked<F, Fut, E>(
    fetch: &F,
    address: Pubkey,
) -> Result<Option<SplAccount>, SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    match fetch(address).await {
        Ok(Some(account)) if account.key != address => Err(SplResolveError::AccountKeyMismatch {
            requested: address,
            returned: account.key,
        }),
        Ok(account) => Ok(account),
        Err(error) => Err(SplResolveError::AccountFetch {
            address,
            source: error.into(),
        }),
    }
}

/// Read and validate everything about a mint's hook except the extras.
/// `Ok(None)` means the mint has no hook.
pub(super) async fn inspect_mint<F, Fut, E>(
    fetch: &F,
    mint: Pubkey,
    options: &ResolveOptions,
) -> Result<Option<Inspection>, SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let mint_account = fetch_checked(fetch, mint)
        .await?
        .ok_or(SplResolveError::MissingMint)?;
    let (hook_program, hook_authority) = if mint_account.owner == spl_token::id() {
        return Ok(None);
    } else if mint_account.owner == spl_token_2022::id() {
        let state = StateWithExtensions::<Token2022Mint>::unpack(&mint_account.data)
            .map_err(|_| SplResolveError::InvalidMintData)?;
        match state.get_extension::<TransferHook>() {
            Ok(extension) => (
                Option::<Pubkey>::from(extension.program_id),
                Option::<Pubkey>::from(extension.authority),
            ),
            Err(_) => (None, None),
        }
    } else {
        return Err(SplResolveError::InvalidMintOwner(mint_account.owner));
    };
    let Some(hook_program) = hook_program else {
        return Ok(None);
    };

    if let Some(expected) = options.expected_hook_program {
        if expected != hook_program {
            return Err(SplResolveError::UnexpectedHookProgram {
                expected,
                found: hook_program,
            });
        }
    }
    if let Some(reason) = invalid_hook_program_reason(&hook_program) {
        return Err(SplResolveError::HookProgramInvalid {
            program: hook_program,
            reason,
        });
    }
    match options.expected_hook_authority {
        AuthorityExpectation::Any => {}
        AuthorityExpectation::Exactly(expected) if hook_authority == Some(expected) => {}
        AuthorityExpectation::Revoked if hook_authority.is_none() => {}
        expected => {
            return Err(SplResolveError::HookAuthorityViolation {
                expected,
                found: hook_authority,
            })
        }
    }

    let (loader, program) = inspect_program(fetch, hook_program, options).await?;

    let list_address = get_extra_account_metas_address(&mint, &hook_program);
    let list_account = fetch_checked(fetch, list_address)
        .await?
        .ok_or(SplResolveError::MissingValidationList(list_address))?;
    if list_account.owner != hook_program {
        return Err(SplResolveError::InvalidValidationListOwner {
            address: list_address,
            owner: list_account.owner,
            expected: hook_program,
        });
    }
    check_validation_list(&list_address, &list_account.data)?;
    let validation_list_hash = hash(&list_account.data).to_bytes();

    Ok(Some(Inspection {
        hook_program,
        list_address,
        list_data: list_account.data,
        fingerprint: HookFingerprint {
            mint,
            hook_program,
            hook_authority,
            loader,
            program,
            validation_list: list_address,
            validation_list_hash,
        },
    }))
}

pub(crate) fn invalid_hook_program_reason(program: &Pubkey) -> Option<HookProgramInvalidReason> {
    if *program == Pubkey::default() {
        Some(HookProgramInvalidReason::Zero)
    } else if *program == spl_token_2022::id() {
        Some(HookProgramInvalidReason::Token2022Program)
    } else if *program == spl_token::id() {
        Some(HookProgramInvalidReason::SplTokenProgram)
    } else if RAYDIUM_PROGRAM_IDS.contains(program) {
        Some(HookProgramInvalidReason::RaydiumProgram)
    } else {
        None
    }
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}

fn read_key(data: &[u8], offset: usize) -> Option<Pubkey> {
    let bytes: [u8; 32] = data.get(offset..offset.checked_add(32)?)?.try_into().ok()?;
    Some(Pubkey::new_from_array(bytes))
}

pub(super) async fn inspect_program<F, Fut, E>(
    fetch: &F,
    program: Pubkey,
    options: &ResolveOptions,
) -> Result<(Pubkey, ProgramFingerprint), SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let account = fetch_checked(fetch, program)
        .await?
        .ok_or(SplResolveError::MissingHookProgram)?;
    if !account.executable {
        return Err(SplResolveError::HookProgramNotExecutable);
    }
    let loader = account.owner;
    if !options.allowed_loaders.contains(&loader) {
        return Err(SplResolveError::HookProgramBadLoader { program, loader });
    }
    let malformed = || SplResolveError::HookProgramInvalid {
        program,
        reason: HookProgramInvalidReason::MalformedProgramAccount,
    };

    if loader == bpf_loader_upgradeable::id() {
        if read_u32(&account.data, 0) != Some(UPGRADEABLE_PROGRAM_TAG)
            || account.data.len() < UPGRADEABLE_PROGRAM_LEN
        {
            return Err(malformed());
        }
        let program_data = read_key(&account.data, 4).ok_or_else(malformed)?;
        let data_account = fetch_checked(fetch, program_data)
            .await?
            .ok_or(SplResolveError::HookProgramClosed { program })?;
        if data_account.owner != bpf_loader_upgradeable::id() {
            return Err(SplResolveError::HookProgramBadLoader {
                program,
                loader: data_account.owner,
            });
        }
        match read_u32(&data_account.data, 0) {
            Some(UPGRADEABLE_PROGRAM_DATA_TAG)
                if data_account.data.len() >= UPGRADEABLE_PROGRAM_DATA_META_LEN => {}
            Some(UPGRADEABLE_UNINITIALIZED_TAG) | None => {
                return Err(SplResolveError::HookProgramClosed { program })
            }
            Some(_) => return Err(malformed()),
        }
        let slot = read_u64(&data_account.data, 4).ok_or_else(malformed)?;
        let upgrade_authority = match data_account.data.get(12) {
            Some(0) => None,
            Some(1) => Some(read_key(&data_account.data, 13).ok_or_else(malformed)?),
            _ => return Err(malformed()),
        };
        Ok((
            loader,
            ProgramFingerprint::Upgradeable {
                program_data,
                slot,
                upgrade_authority,
            },
        ))
    } else if loader == LOADER_V4_ID {
        if account.data.len() < LOADER_V4_HEADER_LEN {
            return Err(malformed());
        }
        let slot = read_u64(&account.data, 0).ok_or_else(malformed)?;
        let authority_or_next_version = read_key(&account.data, 8).ok_or_else(malformed)?;
        let status = read_u64(&account.data, LOADER_V4_STATUS_OFFSET).ok_or_else(malformed)?;
        match status {
            0 => return Err(SplResolveError::HookProgramClosed { program }),
            1 | 2 => {}
            _ => return Err(malformed()),
        }
        Ok((
            loader,
            ProgramFingerprint::LoaderV4 {
                slot,
                authority_or_next_version,
                status,
            },
        ))
    } else {
        Ok((loader, ProgramFingerprint::Immutable))
    }
}

/// The 8-byte TLV discriminator of the SPL Execute instruction.
pub(crate) fn execute_discriminator() -> [u8; 8] {
    let packed = TransferHookInstruction::Execute { amount: 0 }.pack();
    packed[..8]
        .try_into()
        .expect("Execute packs a discriminator")
}

/// Structural check of an `ExtraAccountMetaList` account so a malformed list is
/// reported as such instead of as a generic fetch or resolution failure.
fn check_validation_list(address: &Pubkey, data: &[u8]) -> Result<(), SplResolveError> {
    let malformed = |reason: &str| SplResolveError::ValidationListMalformed {
        address: *address,
        reason: reason.to_string(),
    };
    let discriminator = execute_discriminator();
    let mut offset = 0usize;
    loop {
        let header_end = offset
            .checked_add(TLV_ENTRY_HEADER_LEN)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| malformed("no Execute entry in validation list"))?;
        let length = read_u32(data, offset + 8).ok_or_else(|| malformed("truncated TLV header"))?;
        let value_end = usize::try_from(length)
            .ok()
            .and_then(|length| header_end.checked_add(length))
            .filter(|end| *end <= data.len())
            .ok_or_else(|| malformed("TLV entry length exceeds account data"))?;
        if data[offset..offset + 8] == discriminator {
            let value = &data[header_end..value_end];
            let count =
                read_u32(value, 0).ok_or_else(|| malformed("missing ExtraAccountMeta count"))?;
            let needed = usize::try_from(count)
                .ok()
                .and_then(|count| count.checked_mul(std::mem::size_of::<ExtraAccountMeta>()))
                .and_then(|size| size.checked_add(POD_SLICE_PREFIX_LEN))
                .ok_or_else(|| malformed("ExtraAccountMeta count overflows"))?;
            if needed > value.len() {
                return Err(malformed("ExtraAccountMeta count exceeds entry length"));
            }
            return Ok(());
        }
        offset = value_end;
    }
}
