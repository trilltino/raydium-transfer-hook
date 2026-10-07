use std::{cell::Cell, future::ready};

use solana_program::{
    bpf_loader, bpf_loader_upgradeable,
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};
use spl_transfer_hook_interface::get_extra_account_metas_address;

use super::{cpmm_accounts, setup, Setup};
use crate::{
    abi::build_cpmm_swap_base_input_v1,
    error::{
        AuthorityExpectation, FetchError, HookChangeKind, HookProgramInvalidReason, LegError,
        LegRole, SplResolveError,
    },
    resolve::{
        invalid_hook_program_reason, resolve_leg, resolve_legs, LegHook, PrivilegePolicy,
        ProgramFingerprint, ResolveOptions, SplTransferLeg, LOADER_V4_ID, RAYDIUM_PROGRAM_IDS,
    },
    testing::{token_2022_mint_data, token_2022_plain_mint_data, validation_list_data},
};

async fn resolve(s: &Setup, options: &ResolveOptions) -> Result<LegHook, LegError> {
    resolve_leg(LegRole::Input, s.leg(42), options, s.chain.fetcher()).await
}

async fn resolve_err(s: &Setup, options: &ResolveOptions) -> SplResolveError {
    resolve(s, options)
        .await
        .expect_err("resolution must fail")
        .source
}

fn ro(key: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(key, false)
}

fn pubkey_meta(key: &Pubkey, signer: bool, writable: bool) -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_pubkey(key, signer, writable).unwrap()
}

fn data_seed_meta() -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_seeds(
        &[Seed::AccountData {
            account_index: 2, // destination
            data_index: 0,
            length: 8,
        }],
        false,
        false,
    )
    .unwrap()
}

// ----- N = 0 and unhooked mints -------------------------------------------------

#[tokio::test]
async fn empty_list_resolves_to_hook_program_and_canonical_list_only() {
    let s = setup(&[]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let slice = leg.slice().expect("hooked");
    let list = get_extra_account_metas_address(&s.mint, &s.hook);
    assert_eq!(slice.metas(), [ro(s.hook), ro(list)]);
    assert!(slice.extras().is_empty());
    assert_eq!(slice.hook_program(), s.hook);
    assert_eq!(slice.validation_list(), list);
    assert_eq!(leg.account_count(), 2);
    assert_eq!(leg.role(), LegRole::Input);
    assert_eq!(leg.transfer().amount, 42);
}

#[tokio::test]
async fn classic_and_hookless_mints_resolve_unhooked() {
    let mut s = setup(&[]);
    let classic = Pubkey::new_unique();
    let plain = Pubkey::new_unique();
    let none_program = Pubkey::new_unique();
    s.chain.add_classic_mint(classic);
    s.chain.add_unhooked_token_2022_mint(plain);
    // The extension is present but its program id is unset: Token-2022 skips the hook.
    s.chain.insert(
        none_program,
        spl_token_2022::id(),
        token_2022_mint_data(None, Some(Pubkey::new_unique())),
        false,
    );
    for mint in [classic, plain, none_program] {
        let transfer = SplTransferLeg { mint, ..s.leg(1) };
        let leg = resolve_leg(
            LegRole::Output,
            transfer,
            &ResolveOptions::default(),
            s.chain.fetcher(),
        )
        .await
        .unwrap();
        assert!(!leg.is_hooked());
        assert_eq!(leg.account_count(), 0);
        assert!(leg.fingerprint().is_none());
    }
}

// ----- N > 0 resolution with real lists ------------------------------------------

#[tokio::test]
async fn account_data_seed_resolves_a_pda_from_instruction_account_data() {
    let mut s = setup(&[data_seed_meta()]);
    s.chain
        .insert(s.destination, spl_token_2022::id(), vec![7; 40], false);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let slice = leg.slice().unwrap();
    let expected = Pubkey::find_program_address(&[&[7u8; 8]], &s.hook).0;
    assert_eq!(slice.metas().len(), 3);
    assert_eq!(slice.extras(), [ro(expected)]);
    assert_eq!(slice.metas()[1], ro(s.hook));
}

#[tokio::test]
async fn instruction_data_seed_resolves_a_pda_from_the_transfer_amount() {
    let s = setup(&[ExtraAccountMeta::new_with_seeds(
        &[Seed::InstructionData {
            index: 8,
            length: 8,
        }],
        false,
        false,
    )
    .unwrap()]);
    let amount = 0x0102_0304_0506_0708u64;
    let leg = resolve_leg(
        LegRole::Input,
        s.leg(amount),
        &ResolveOptions::default(),
        s.chain.fetcher(),
    )
    .await
    .unwrap();
    let expected = Pubkey::find_program_address(&[&amount.to_le_bytes()], &s.hook).0;
    assert_eq!(leg.slice().unwrap().extras(), [ro(expected)]);

    // A different amount derives a different account: nothing is cached.
    let other = resolve_leg(
        LegRole::Input,
        s.leg(amount + 1),
        &ResolveOptions::default(),
        s.chain.fetcher(),
    )
    .await
    .unwrap();
    assert_ne!(
        other.slice().unwrap().extras(),
        leg.slice().unwrap().extras()
    );
}

#[tokio::test]
async fn external_program_pda_uses_the_program_at_the_given_account_index() {
    let other_program = Pubkey::new_unique();
    let s = setup(&[
        pubkey_meta(&other_program, false, false),
        ExtraAccountMeta::new_external_pda_with_seeds(
            5, // the extra account resolved just above
            &[Seed::Literal {
                bytes: b"ext".to_vec(),
            }],
            false,
            false,
        )
        .unwrap(),
    ]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let expected = Pubkey::find_program_address(&[b"ext"], &other_program).0;
    assert_eq!(
        leg.slice().unwrap().extras(),
        [ro(other_program), ro(expected)]
    );
}

#[tokio::test]
async fn account_key_seed_and_literal_seed_combine() {
    let s = setup(&[ExtraAccountMeta::new_with_seeds(
        &[
            Seed::Literal {
                bytes: b"vault".to_vec(),
            },
            Seed::AccountKey { index: 0 }, // source
        ],
        false,
        false,
    )
    .unwrap()]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let expected = Pubkey::find_program_address(&[b"vault", s.source.as_ref()], &s.hook).0;
    assert_eq!(leg.slice().unwrap().extras(), [ro(expected)]);
}

// ----- one test per error variant --------------------------------------------------

#[tokio::test]
async fn error_account_fetch_is_attributed_to_the_failing_address() {
    let mut s = setup(&[]);
    s.chain.failing.insert(s.mint);
    match resolve_err(&s, &ResolveOptions::default()).await {
        SplResolveError::AccountFetch { address, source } => {
            assert_eq!(address, s.mint);
            assert!(source.message().contains("rpc failure"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn error_account_fetch_inside_extra_resolution_keeps_its_address() {
    let mut s = setup(&[data_seed_meta()]);
    s.chain.failing.insert(s.destination);
    match resolve_err(&s, &ResolveOptions::default()).await {
        SplResolveError::AccountFetch { address, .. } => assert_eq!(address, s.destination),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn error_invalid_mint_owner() {
    let mut s = setup(&[]);
    let owner = Pubkey::new_unique();
    s.chain.insert(s.mint, owner, vec![0; 82], false);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::InvalidMintOwner(owner)
    );
}

#[tokio::test]
async fn error_account_key_mismatch() {
    let s = setup(&[]);
    let impostor = Pubkey::new_unique();
    let chain = &s.chain;
    let fetch = |address: Pubkey| {
        let mut account = chain.accounts.get(&address).cloned();
        if address == s.mint {
            if let Some(account) = account.as_mut() {
                account.key = impostor;
            }
        }
        ready(Ok::<_, FetchError>(account))
    };
    let error = resolve_leg(LegRole::Input, s.leg(1), &ResolveOptions::default(), fetch)
        .await
        .unwrap_err();
    assert_eq!(
        error.source,
        SplResolveError::AccountKeyMismatch {
            requested: s.mint,
            returned: impostor
        }
    );
}

#[tokio::test]
async fn error_missing_mint() {
    let mut s = setup(&[]);
    s.chain.accounts.remove(&s.mint);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::MissingMint
    );
}

#[tokio::test]
async fn error_invalid_mint_data() {
    let mut s = setup(&[]);
    s.chain
        .insert(s.mint, spl_token_2022::id(), vec![1, 2, 3], false);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::InvalidMintData
    );
}

#[tokio::test]
async fn error_missing_hook_program() {
    let mut s = setup(&[]);
    s.chain.accounts.remove(&s.hook);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::MissingHookProgram
    );
}

#[tokio::test]
async fn error_hook_program_not_executable() {
    let mut s = setup(&[]);
    s.chain.insert(s.hook, bpf_loader::id(), Vec::new(), false);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramNotExecutable
    );
}

#[tokio::test]
async fn error_hook_program_invalid_for_token_and_raydium_programs() {
    for (program, reason) in [
        (
            spl_token_2022::id(),
            HookProgramInvalidReason::Token2022Program,
        ),
        (spl_token::id(), HookProgramInvalidReason::SplTokenProgram),
        (
            RAYDIUM_PROGRAM_IDS[0],
            HookProgramInvalidReason::RaydiumProgram,
        ),
        (
            RAYDIUM_PROGRAM_IDS[2],
            HookProgramInvalidReason::RaydiumProgram,
        ),
    ] {
        let mut s = setup(&[]);
        s.chain.add_hooked_mint(s.mint, program, None, &[]);
        assert_eq!(
            resolve_err(&s, &ResolveOptions::default()).await,
            SplResolveError::HookProgramInvalid { program, reason }
        );
    }
}

#[test]
fn zero_hook_program_is_rejected_by_the_guard() {
    // A mint's OptionalNonZeroPubkey encodes the zero key as "none", so a zero
    // hook program reads as unhooked and never reaches the guard through a
    // mint. The guard is defense in depth and is tested directly.
    assert_eq!(
        invalid_hook_program_reason(&Pubkey::default()),
        Some(HookProgramInvalidReason::Zero)
    );
    assert_eq!(invalid_hook_program_reason(&Pubkey::new_unique()), None);
}

#[tokio::test]
async fn error_hook_program_bad_loader() {
    let mut s = setup(&[]);
    let loader = Pubkey::new_unique();
    s.chain.insert(s.hook, loader, Vec::new(), true);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramBadLoader {
            program: s.hook,
            loader
        }
    );
    // The same program is accepted when the caller allows its loader.
    let options = ResolveOptions::default().with_allowed_loaders(vec![loader]);
    assert!(resolve(&s, &options).await.is_ok());
}

#[tokio::test]
async fn error_hook_program_closed_for_missing_and_uninitialized_program_data() {
    let mut s = setup(&[]);
    let program_data = Pubkey::new_unique();
    s.chain
        .add_upgradeable_program(s.hook, program_data, 9, Some(Pubkey::new_unique()));
    assert!(resolve(&s, &ResolveOptions::default()).await.is_ok());

    let removed = s.chain.accounts.remove(&program_data).unwrap();
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramClosed { program: s.hook }
    );

    let uninitialized = vec![0u8; 45];
    s.chain
        .insert(program_data, removed.owner, uninitialized, false);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramClosed { program: s.hook }
    );
}

#[tokio::test]
async fn upgradeable_program_data_owner_and_shape_are_checked() {
    let mut s = setup(&[]);
    let program_data = Pubkey::new_unique();
    s.chain
        .add_upgradeable_program(s.hook, program_data, 9, None);
    let program_data_account = s.chain.accounts[&program_data].clone();

    let rogue = Pubkey::new_unique();
    s.chain
        .insert(program_data, rogue, program_data_account.data, false);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramBadLoader {
            program: s.hook,
            loader: rogue
        }
    );

    s.chain.insert(
        program_data,
        bpf_loader_upgradeable::id(),
        vec![9; 64],
        false,
    );
    assert!(matches!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramInvalid {
            reason: HookProgramInvalidReason::MalformedProgramAccount,
            ..
        }
    ));

    s.chain
        .insert(s.hook, bpf_loader_upgradeable::id(), vec![1; 10], true);
    assert!(matches!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramInvalid {
            reason: HookProgramInvalidReason::MalformedProgramAccount,
            ..
        }
    ));
}

fn loader_v4_data(slot: u64, authority: Pubkey, status: u64) -> Vec<u8> {
    let mut data = slot.to_le_bytes().to_vec();
    data.extend_from_slice(authority.as_ref());
    data.extend_from_slice(&status.to_le_bytes());
    data.extend_from_slice(&[0xBB; 8]);
    data
}

#[tokio::test]
async fn loader_v4_programs_must_be_deployed() {
    let mut s = setup(&[]);
    let authority = Pubkey::new_unique();
    s.chain
        .insert(s.hook, LOADER_V4_ID, loader_v4_data(3, authority, 1), true);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    assert_eq!(
        leg.fingerprint().unwrap().program,
        ProgramFingerprint::LoaderV4 {
            slot: 3,
            authority_or_next_version: authority,
            status: 1
        }
    );

    s.chain
        .insert(s.hook, LOADER_V4_ID, loader_v4_data(3, authority, 0), true);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramClosed { program: s.hook }
    );
    s.chain
        .insert(s.hook, LOADER_V4_ID, loader_v4_data(3, authority, 77), true);
    assert!(matches!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramInvalid {
            reason: HookProgramInvalidReason::MalformedProgramAccount,
            ..
        }
    ));
    s.chain.insert(s.hook, LOADER_V4_ID, vec![0; 10], true);
    assert!(matches!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::HookProgramInvalid { .. }
    ));
}

#[tokio::test]
async fn error_unexpected_hook_program() {
    let s = setup(&[]);
    let expected = Pubkey::new_unique();
    let options = ResolveOptions::default().with_expected_hook_program(expected);
    assert_eq!(
        resolve_err(&s, &options).await,
        SplResolveError::UnexpectedHookProgram {
            expected,
            found: s.hook
        }
    );
    let matching = ResolveOptions::default().with_expected_hook_program(s.hook);
    assert!(resolve(&s, &matching).await.is_ok());
}

#[tokio::test]
async fn error_hook_required_for_unhooked_mints() {
    let mut s = setup(&[]);
    s.chain.add_unhooked_token_2022_mint(s.mint);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default().requiring_hook()).await,
        SplResolveError::HookRequired
    );
    // An expected program also implies a hook must be present.
    assert_eq!(
        resolve_err(
            &s,
            &ResolveOptions::default().with_expected_hook_program(Pubkey::new_unique())
        )
        .await,
        SplResolveError::HookRequired
    );
    assert!(resolve(&s, &ResolveOptions::default()).await.is_ok());
}

#[tokio::test]
async fn error_hook_authority_violation() {
    let s = setup(&[]);
    let expected = Pubkey::new_unique();
    let wants_key = ResolveOptions::default()
        .with_expected_hook_authority(AuthorityExpectation::Exactly(expected));
    match resolve_err(&s, &wants_key).await {
        SplResolveError::HookAuthorityViolation {
            expected: AuthorityExpectation::Exactly(key),
            found: Some(_),
        } => assert_eq!(key, expected),
        other => panic!("unexpected {other:?}"),
    }
    let revoked =
        ResolveOptions::default().with_expected_hook_authority(AuthorityExpectation::Revoked);
    assert!(matches!(
        resolve_err(&s, &revoked).await,
        SplResolveError::HookAuthorityViolation {
            expected: AuthorityExpectation::Revoked,
            found: Some(_)
        }
    ));

    let mut renounced = setup(&[]);
    renounced
        .chain
        .add_hooked_mint(renounced.mint, renounced.hook, None, &[]);
    assert!(resolve(&renounced, &revoked).await.is_ok());
    assert!(matches!(
        resolve_err(&renounced, &wants_key).await,
        SplResolveError::HookAuthorityViolation { found: None, .. }
    ));
}

#[tokio::test]
async fn error_missing_validation_list() {
    let mut s = setup(&[]);
    let list = get_extra_account_metas_address(&s.mint, &s.hook);
    s.chain.accounts.remove(&list);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::MissingValidationList(list)
    );
}

#[tokio::test]
async fn error_invalid_validation_list_owner() {
    let mut s = setup(&[]);
    let list = get_extra_account_metas_address(&s.mint, &s.hook);
    let data = s.chain.accounts[&list].data.clone();
    let rogue = Pubkey::new_unique();
    s.chain.insert(list, rogue, data, false);
    assert_eq!(
        resolve_err(&s, &ResolveOptions::default()).await,
        SplResolveError::InvalidValidationListOwner {
            address: list,
            owner: rogue,
            expected: s.hook
        }
    );
}

#[tokio::test]
async fn error_validation_list_malformed_variants() {
    let valid = validation_list_data(&[pubkey_meta(&Pubkey::new_unique(), false, false)]);
    let mut wrong_discriminator = valid.clone();
    wrong_discriminator[..8].fill(0xEE);
    let mut huge_count = valid.clone();
    huge_count[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut huge_length = valid.clone();
    huge_length[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let cases = [
        Vec::new(),
        vec![0; 4],
        vec![0; 64],
        wrong_discriminator,
        huge_count,
        huge_length,
        valid[..valid.len() - 10].to_vec(),
    ];
    for data in cases {
        let mut s = setup(&[]);
        s.chain.set_validation_list(s.mint, s.hook, data.clone());
        let list = get_extra_account_metas_address(&s.mint, &s.hook);
        match resolve_err(&s, &ResolveOptions::default()).await {
            SplResolveError::ValidationListMalformed { address, reason } => {
                assert_eq!(address, list, "case len {}", data.len());
                assert!(!reason.is_empty());
            }
            other => panic!("unexpected {other:?} for list of {} bytes", data.len()),
        }
    }
}

#[tokio::test]
async fn error_extra_account_resolution_for_out_of_range_instruction_data_seed() {
    let s = setup(&[ExtraAccountMeta::new_with_seeds(
        &[Seed::InstructionData {
            index: 200,
            length: 8,
        }],
        false,
        false,
    )
    .unwrap()]);
    match resolve_err(&s, &ResolveOptions::default()).await {
        SplResolveError::ExtraAccountResolution { code, reason } => {
            assert!(matches!(code, Some(ProgramError::Custom(_))));
            assert!(!reason.is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn privilege_policy_rejects_signers_and_writables_by_default() {
    let wallet = Pubkey::new_unique();
    let signer = setup(&[pubkey_meta(&wallet, true, false)]);
    assert_eq!(
        resolve_err(&signer, &ResolveOptions::default()).await,
        SplResolveError::UnexpectedSigner { address: wallet }
    );
    let writable = setup(&[pubkey_meta(&wallet, false, true)]);
    assert_eq!(
        resolve_err(&writable, &ResolveOptions::default()).await,
        SplResolveError::UnexpectedWritable { address: wallet }
    );

    // Allow lists are per account.
    let allowed = ResolveOptions::default()
        .with_privilege_policy(PrivilegePolicy::allowing_writable([wallet]));
    let leg = resolve(&writable, &allowed).await.unwrap();
    assert!(leg.slice().unwrap().extras()[0].is_writable);
    let other = ResolveOptions::default()
        .with_privilege_policy(PrivilegePolicy::allowing_writable([Pubkey::new_unique()]));
    assert!(resolve(&writable, &other).await.is_err());
    let signer_ok = ResolveOptions::default().with_privilege_policy(PrivilegePolicy {
        allowed_signers: vec![wallet],
        ..PrivilegePolicy::default()
    });
    assert!(resolve(&signer, &signer_ok).await.is_ok());
    let trusted =
        ResolveOptions::default().with_privilege_policy(PrivilegePolicy::trust_everything());
    assert!(resolve(&signer, &trusted).await.is_ok());
}

#[tokio::test]
async fn the_resolved_tail_is_readonly_and_a_hostile_extra_naming_the_hook_is_policed() {
    let s = setup(&[]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let metas = leg.slice().unwrap().metas();
    for meta in &metas[metas.len() - 2..] {
        assert!(!meta.is_signer && !meta.is_writable);
    }
    let hook = Pubkey::new_unique();
    let mut hostile = setup(&[]);
    hostile
        .chain
        .add_hooked_mint(hostile.mint, hook, None, &[pubkey_meta(&hook, false, true)]);
    assert!(matches!(
        resolve_err(&hostile, &ResolveOptions::default()).await,
        SplResolveError::UnexpectedWritable { .. }
    ));
}

// ----- change detection -------------------------------------------------------------

#[tokio::test]
async fn error_validation_list_changed_between_reads_during_resolution() {
    let s = setup(&[]);
    let list = get_extra_account_metas_address(&s.mint, &s.hook);
    let changed = validation_list_data(&[pubkey_meta(&Pubkey::new_unique(), false, false)]);
    let list_reads = Cell::new(0u32);
    let chain = &s.chain;
    let racing_fetch = |address: Pubkey| {
        let mut account = chain.accounts.get(&address).cloned();
        if address == list {
            list_reads.set(list_reads.get() + 1);
            if list_reads.get() > 1 {
                account.as_mut().unwrap().data = changed.clone();
            }
        }
        ready(Ok::<_, FetchError>(account))
    };
    let error = resolve_leg(
        LegRole::Input,
        s.leg(1),
        &ResolveOptions::default(),
        racing_fetch,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error.source,
        SplResolveError::ValidationListChanged { address, .. } if address == list
    ));

    // With the recheck disabled the race is not detected at resolution time.
    list_reads.set(0);
    let options = ResolveOptions {
        recheck_consistency: false,
        ..ResolveOptions::default()
    };
    let racing_fetch = |address: Pubkey| {
        let mut account = chain.accounts.get(&address).cloned();
        if address == list {
            list_reads.set(list_reads.get() + 1);
            if list_reads.get() > 1 {
                account.as_mut().unwrap().data = changed.clone();
            }
        }
        ready(Ok::<_, FetchError>(account))
    };
    assert!(
        resolve_leg(LegRole::Input, s.leg(1), &options, racing_fetch)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn verify_unchanged_accepts_an_untouched_hook() {
    let s = setup(&[]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    leg.verify_unchanged(s.chain.fetcher()).await.unwrap();
    leg.slice()
        .unwrap()
        .fingerprint()
        .verify_unchanged(s.chain.fetcher())
        .await
        .unwrap();
    crate::verify_legs_unchanged(&[&leg], s.chain.fetcher())
        .await
        .unwrap();
}

#[tokio::test]
async fn verify_unchanged_detects_a_changed_validation_list() {
    let mut s = setup(&[]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let list = get_extra_account_metas_address(&s.mint, &s.hook);
    s.chain.set_validation_list(
        s.mint,
        s.hook,
        validation_list_data(&[pubkey_meta(&Pubkey::new_unique(), false, false)]),
    );
    let error = leg.verify_unchanged(s.chain.fetcher()).await.unwrap_err();
    assert_eq!(error.leg, LegRole::Input);
    assert_eq!(error.mint, s.mint);
    assert!(matches!(
        error.source,
        SplResolveError::ValidationListChanged { address, before, after }
            if address == list && before != after
    ));
}

#[tokio::test]
async fn verify_unchanged_detects_a_program_upgrade() {
    let mut s = setup(&[]);
    let program_data = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    s.chain
        .add_upgradeable_program(s.hook, program_data, 10, Some(authority));
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    leg.verify_unchanged(s.chain.fetcher()).await.unwrap();

    s.chain
        .add_upgradeable_program(s.hook, program_data, 11, Some(authority));
    assert_eq!(
        leg.verify_unchanged(s.chain.fetcher())
            .await
            .unwrap_err()
            .source,
        SplResolveError::HookProgramChanged {
            program: s.hook,
            kind: HookChangeKind::ProgramStateChanged
        }
    );

    s.chain
        .add_upgradeable_program(s.hook, program_data, 10, Some(Pubkey::new_unique()));
    assert!(matches!(
        leg.verify_unchanged(s.chain.fetcher())
            .await
            .unwrap_err()
            .source,
        SplResolveError::HookProgramChanged {
            kind: HookChangeKind::ProgramStateChanged,
            ..
        }
    ));
}

#[tokio::test]
async fn verify_unchanged_detects_repointed_removed_and_reauthorized_mints() {
    let mut s = setup(&[]);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let original_authority = leg.fingerprint().unwrap().hook_authority.unwrap();

    let new_authority = Pubkey::new_unique();
    s.chain
        .add_hooked_mint(s.mint, s.hook, Some(new_authority), &[]);
    assert_eq!(
        leg.verify_unchanged(s.chain.fetcher())
            .await
            .unwrap_err()
            .source,
        SplResolveError::HookAuthorityViolation {
            expected: AuthorityExpectation::Exactly(original_authority),
            found: Some(new_authority)
        }
    );

    let other_hook = Pubkey::new_unique();
    s.chain.add_hooked_mint(s.mint, other_hook, None, &[]);
    assert_eq!(
        leg.verify_unchanged(s.chain.fetcher())
            .await
            .unwrap_err()
            .source,
        SplResolveError::HookProgramChanged {
            program: s.hook,
            kind: HookChangeKind::MintRepointed(other_hook)
        }
    );

    s.chain.add_unhooked_token_2022_mint(s.mint);
    assert_eq!(
        leg.verify_unchanged(s.chain.fetcher())
            .await
            .unwrap_err()
            .source,
        SplResolveError::HookProgramChanged {
            program: s.hook,
            kind: HookChangeKind::HookRemoved
        }
    );
}

#[tokio::test]
async fn unhooked_legs_verify_that_the_mint_stays_unhooked() {
    let mut s = setup(&[]);
    s.chain.add_unhooked_token_2022_mint(s.mint);
    let leg = resolve(&s, &ResolveOptions::default()).await.unwrap();
    assert!(!leg.is_hooked());
    leg.verify_unchanged(s.chain.fetcher()).await.unwrap();
    s.chain.add_hooked_mint(s.mint, s.hook, None, &[]);
    assert!(matches!(
        leg.verify_unchanged(s.chain.fetcher())
            .await
            .unwrap_err()
            .source,
        SplResolveError::HookProgramChanged {
            kind: HookChangeKind::MintRepointed(_),
            ..
        }
    ));
}

#[tokio::test]
async fn every_resolution_reads_the_chain_afresh() {
    let mut s = setup(&[]);
    let first = resolve(&s, &ResolveOptions::default()).await.unwrap();
    let reads_for_one = s.chain.fetch_count();
    assert!(reads_for_one >= 3);
    let again = resolve(&s, &ResolveOptions::default()).await.unwrap();
    assert_eq!(s.chain.fetch_count(), reads_for_one * 2);
    assert_eq!(first, again);

    s.chain.set_validation_list(
        s.mint,
        s.hook,
        validation_list_data(&[pubkey_meta(&Pubkey::new_unique(), false, false)]),
    );
    let changed = resolve(&s, &ResolveOptions::default()).await.unwrap();
    assert_ne!(first, changed);
    assert_eq!(changed.account_count(), 3);
}

// ----- atomicity and independence ----------------------------------------------------

#[tokio::test]
async fn batch_failure_attributes_the_failing_leg_and_leaves_the_instruction_untouched() {
    let s = setup(&[]);
    let missing_mint = Pubkey::new_unique();
    let accounts = cpmm_accounts();
    let instruction = build_cpmm_swap_base_input_v1(Pubkey::new_unique(), &accounts, 10, 1);
    let original: Instruction = instruction.clone();

    let legs = [
        (LegRole::Input, s.leg(10)),
        (
            LegRole::Output,
            SplTransferLeg {
                mint: missing_mint,
                ..s.leg(9)
            },
        ),
    ];
    let error = resolve_legs(&legs, &ResolveOptions::default(), s.chain.fetcher())
        .await
        .unwrap_err();
    assert_eq!(error.leg, LegRole::Output);
    assert_eq!(error.mint, missing_mint);
    assert_eq!(error.source, SplResolveError::MissingMint);
    assert!(error.to_string().contains("output leg"));
    // Resolution never takes the caller's instruction, so a leg-2 failure cannot
    // leave leg 1's accounts appended to it.
    assert_eq!(instruction, original);
}

#[tokio::test]
async fn batch_resolution_returns_legs_in_order_with_their_roles() {
    let mut s = setup(&[]);
    let mint_b = Pubkey::new_unique();
    let hook_b = Pubkey::new_unique();
    s.chain.add_hooked_mint(mint_b, hook_b, None, &[]);
    let legs = [
        (LegRole::Token0, s.leg(1)),
        (
            LegRole::Token1,
            SplTransferLeg {
                mint: mint_b,
                ..s.leg(2)
            },
        ),
    ];
    let resolved = resolve_legs(&legs, &ResolveOptions::default(), s.chain.fetcher())
        .await
        .unwrap();
    assert_eq!(resolved[0].role(), LegRole::Token0);
    assert_eq!(resolved[1].role(), LegRole::Token1);
    assert_eq!(resolved[0].slice().unwrap().hook_program(), s.hook);
    assert_eq!(resolved[1].slice().unwrap().hook_program(), hook_b);
}

#[tokio::test]
async fn second_leg_with_a_different_list_gets_its_own_independent_slice() {
    let first_extra = Pubkey::new_unique();
    let second_extra = Pubkey::new_unique();
    let mut s = setup(&[pubkey_meta(&first_extra, false, false)]);
    let mint_b = Pubkey::new_unique();
    let hook_b = Pubkey::new_unique();
    s.chain.add_hooked_mint(
        mint_b,
        hook_b,
        None,
        &[
            pubkey_meta(&second_extra, false, false),
            pubkey_meta(&first_extra, false, false),
        ],
    );
    let legs = [
        (LegRole::Input, s.leg(5)),
        (
            LegRole::Output,
            SplTransferLeg {
                mint: mint_b,
                ..s.leg(6)
            },
        ),
    ];
    let resolved = resolve_legs(&legs, &ResolveOptions::default(), s.chain.fetcher())
        .await
        .unwrap();
    let (input, output) = (resolved[0].slice().unwrap(), resolved[1].slice().unwrap());
    assert_eq!(input.metas().len(), 3);
    assert_eq!(output.metas().len(), 4);
    assert_eq!(input.extras(), [ro(first_extra)]);
    assert_eq!(output.extras(), [ro(second_extra), ro(first_extra)]);
    assert_ne!(input.validation_list(), output.validation_list());
    assert_ne!(input.hook_program(), output.hook_program());
}

// ----- options ---------------------------------------------------------------------

#[test]
fn policy_decisions_map_onto_resolve_options() {
    use hook_policy_model::{HookAuthorityPolicy, PolicyDecision};
    let hook = [9u8; 32];
    let options = ResolveOptions::from_policy_decision(&PolicyDecision {
        hook_program: Some(hook),
        hook_required: true,
        authority_policy: HookAuthorityPolicy::ImmutableAtLaunch,
    });
    assert_eq!(
        options.expected_hook_program,
        Some(Pubkey::new_from_array(hook))
    );
    assert!(options.require_hook);
    assert_eq!(
        options.expected_hook_authority,
        AuthorityExpectation::Revoked
    );

    let none = ResolveOptions::from_policy_decision(&PolicyDecision {
        hook_program: None,
        hook_required: false,
        authority_policy: HookAuthorityPolicy::ImmutableAtLaunch,
    });
    assert_eq!(none.expected_hook_program, None);
    assert!(!none.require_hook);
    assert_eq!(none.expected_hook_authority, AuthorityExpectation::Any);
}

#[test]
fn the_execute_discriminator_matches_the_spl_interface() {
    use spl_transfer_hook_interface::NAMESPACE;
    let expected = solana_program::hash::hash(format!("{NAMESPACE}:execute").as_bytes());
    assert_eq!(
        crate::resolve::execute_discriminator(),
        expected.to_bytes()[..8]
    );
}

#[test]
fn plain_mint_helper_is_a_valid_token_2022_mint() {
    use spl_token_2022::{extension::StateWithExtensions, state::Mint};
    StateWithExtensions::<Mint>::unpack(&token_2022_plain_mint_data()).unwrap();
}
