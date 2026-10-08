//! Resolving one transfer leg, or several, atomically.

use std::{cell::RefCell, future::Future};

use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_transfer_hook_interface::offchain::add_extra_account_metas_for_execute;

use super::{
    accounts::{SplAccount, SplTransferLeg},
    fingerprint::diff_fingerprint,
    inspect::{fetch_checked, inspect_mint},
    options::ResolveOptions,
    slice::{HookSlice, LegHook},
};
use crate::error::{FetchError, HookChangeKind, LegError, LegRole, SplResolveError};

/// Resolve every leg and fail atomically if any one fails.
pub async fn resolve_legs<F, Fut, E>(
    legs: &[(LegRole, SplTransferLeg)],
    options: &ResolveOptions,
    fetch: F,
) -> Result<Vec<LegHook>, LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let mut resolved = Vec::with_capacity(legs.len());
    for (role, leg) in legs {
        resolved.push(resolve_leg(*role, *leg, options, &fetch).await?);
    }
    Ok(resolved)
}

/// Resolve the hook tail of one transfer on a private scratch instruction.
///
/// The fetcher is called afresh for every account; nothing is cached between
/// calls. A mint without a hook (classic SPL Token, or Token-2022 with no
/// hook program set) yields an unhooked [`LegHook`] unless the options require one.
pub async fn resolve_leg<F, Fut, E>(
    role: LegRole,
    leg: SplTransferLeg,
    options: &ResolveOptions,
    fetch: F,
) -> Result<LegHook, LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let attribute = |source| LegError {
        leg: role,
        mint: leg.mint,
        source,
    };
    resolve_leg_inner(&leg, options, &fetch)
        .await
        .map(|slice| LegHook::new(role, leg, slice))
        .map_err(attribute)
}

async fn resolve_leg_inner<F, Fut, E>(
    leg: &SplTransferLeg,
    options: &ResolveOptions,
    fetch: &F,
) -> Result<Option<HookSlice>, SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let Some(inspection) = inspect_mint(fetch, leg.mint, options).await? else {
        if options.require_hook || options.expected_hook_program.is_some() {
            return Err(SplResolveError::HookRequired);
        }
        return Ok(None);
    };

    let mut scratch = Instruction {
        program_id: spl_token_2022::id(),
        accounts: vec![
            AccountMeta::new(leg.source, false),
            AccountMeta::new_readonly(leg.mint, false),
            AccountMeta::new(leg.destination, false),
            AccountMeta::new_readonly(leg.authority, false),
        ],
        data: Vec::new(),
    };
    let base_len = scratch.accounts.len();
    let fetch_failure: RefCell<Option<SplResolveError>> = RefCell::new(None);
    let list_address = inspection.list_address;
    let list_data = &inspection.list_data;

    let outcome = add_extra_account_metas_for_execute(
        &mut scratch,
        &inspection.hook_program,
        &leg.source,
        &leg.mint,
        &leg.destination,
        &leg.authority,
        leg.amount,
        |address| {
            let fetch_failure = &fetch_failure;
            async move {
                if address == list_address {
                    return Ok(Some(list_data.clone()));
                }
                match fetch_checked(fetch, address).await {
                    Ok(account) => Ok(account.map(|account| account.data)),
                    Err(error) => {
                        fetch_failure.borrow_mut().get_or_insert(error.clone());
                        Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
                    }
                }
            }
        },
    )
    .await;
    if let Err(error) = outcome {
        if let Some(recorded) = fetch_failure.into_inner() {
            return Err(recorded);
        }
        let code = error.downcast_ref::<ProgramError>().cloned();
        return Err(SplResolveError::ExtraAccountResolution {
            code,
            reason: error.to_string(),
        });
    }

    let appended: Vec<AccountMeta> = scratch.accounts.split_off(base_len);
    let tail_ok = appended.len() >= 2
        && appended[appended.len() - 2]
            == AccountMeta::new_readonly(inspection.hook_program, false)
        && appended[appended.len() - 1] == AccountMeta::new_readonly(list_address, false);
    if !tail_ok {
        return Err(SplResolveError::ExtraAccountResolution {
            code: None,
            reason: "resolver did not end with the hook program and validation list".into(),
        });
    }
    for extra in &appended[..appended.len() - 2] {
        options.privilege_policy.check(extra)?;
    }

    if options.recheck_consistency {
        let recheck_options = ResolveOptions {
            allowed_loaders: vec![inspection.fingerprint.loader],
            ..ResolveOptions::default()
        };
        match inspect_mint(fetch, leg.mint, &recheck_options).await? {
            Some(again) => diff_fingerprint(&inspection.fingerprint, &again.fingerprint)?,
            None => {
                return Err(SplResolveError::HookProgramChanged {
                    program: inspection.hook_program,
                    kind: HookChangeKind::HookRemoved,
                })
            }
        }
    }

    Ok(Some(HookSlice::new(
        appended,
        inspection.hook_program,
        list_address,
        inspection.fingerprint,
    )))
}
