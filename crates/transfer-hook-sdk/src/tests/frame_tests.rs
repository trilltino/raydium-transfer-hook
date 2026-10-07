use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::account::ExtraAccountMeta;
use spl_transfer_hook_interface::get_extra_account_metas_address;

use super::{clmm_accounts, cpmm_accounts};
use crate::{
    abi::{
        build_clmm_swap_v2, build_cpmm_swap_base_input_v1, ClmmSwapAccounts, ClmmSwapArgs,
        CpmmSwapAccounts, CLMM_SWAP_V2_DISCRIMINATOR, CLMM_SWAP_V3_DISCRIMINATOR,
        CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
    },
    error::{ConflictSite, FrameError, LegField, LegRole, SliceFault},
    frame::{
        frame_clmm_or_passthrough, frame_clmm_swap_v3, frame_cpmm_or_passthrough,
        frame_cpmm_swap_base_input_v2, FramedAbi,
    },
    resolve::{
        resolve_leg, HookFingerprint, HookSlice, LegHook, ProgramFingerprint, ResolveOptions,
        SplTransferLeg,
    },
    testing::MemoryChain,
};

const PROGRAM: Pubkey = Pubkey::new_from_array([42; 32]);

fn ro(key: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(key, false)
}

fn extra(key: &Pubkey) -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_pubkey(key, false, false).unwrap()
}

struct Cpmm {
    accounts: CpmmSwapAccounts,
    chain: MemoryChain,
    input_leg: SplTransferLeg,
    output_leg: SplTransferLeg,
}

fn cpmm(
    input_extras: Option<&[ExtraAccountMeta]>,
    output_extras: Option<&[ExtraAccountMeta]>,
) -> Cpmm {
    let accounts = cpmm_accounts();
    let mut chain = MemoryChain::new();
    for (mint, extras) in [
        (accounts.input_token_mint, input_extras),
        (accounts.output_token_mint, output_extras),
    ] {
        match extras {
            Some(extras) => chain.add_hooked_mint(mint, Pubkey::new_unique(), None, extras),
            None => chain.add_classic_mint(mint),
        }
    }
    let input_leg = SplTransferLeg {
        source: accounts.input_token_account,
        mint: accounts.input_token_mint,
        destination: accounts.input_vault,
        authority: accounts.payer,
        amount: 1_000,
    };
    let output_leg = SplTransferLeg {
        source: accounts.output_vault,
        mint: accounts.output_token_mint,
        destination: accounts.output_token_account,
        authority: accounts.authority,
        amount: 900,
    };
    Cpmm {
        accounts,
        chain,
        input_leg,
        output_leg,
    }
}

impl Cpmm {
    fn instruction(&self) -> Instruction {
        build_cpmm_swap_base_input_v1(PROGRAM, &self.accounts, 1_000, 1)
    }

    async fn legs(&self) -> (LegHook, LegHook) {
        let options = ResolveOptions::default();
        let input = resolve_leg(
            LegRole::Input,
            self.input_leg,
            &options,
            self.chain.fetcher(),
        )
        .await
        .unwrap();
        let output = resolve_leg(
            LegRole::Output,
            self.output_leg,
            &options,
            self.chain.fetcher(),
        )
        .await
        .unwrap();
        (input, output)
    }
}

fn dummy_fingerprint(mint: Pubkey, hook: Pubkey, list: Pubkey) -> HookFingerprint {
    HookFingerprint {
        mint,
        hook_program: hook,
        hook_authority: None,
        loader: solana_program::bpf_loader::id(),
        program: ProgramFingerprint::Immutable,
        validation_list: list,
        validation_list_hash: [0; 32],
    }
}

/// A leg whose slice was NOT produced by the resolver, to exercise the
/// framer's defense-in-depth checks. Only possible inside this crate.
fn forged(
    role: LegRole,
    transfer: SplTransferLeg,
    metas: Vec<AccountMeta>,
    hook: Pubkey,
    list: Pubkey,
) -> LegHook {
    let fingerprint = dummy_fingerprint(transfer.mint, hook, list);
    LegHook::new(
        role,
        transfer,
        Some(HookSlice::new(metas, hook, list, fingerprint)),
    )
}

fn well_formed_tail(mint: &Pubkey, hook: Pubkey) -> (Vec<AccountMeta>, Pubkey) {
    let list = get_extra_account_metas_address(mint, &hook);
    (vec![ro(hook), ro(list)], list)
}

// ----- CPMM --------------------------------------------------------------------------

#[tokio::test]
async fn cpmm_frames_v1_to_v2_with_distinct_slices_in_order() {
    let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
    let fixture = cpmm(Some(&[extra(&a)]), Some(&[extra(&a), extra(&b)]));
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction();
    let before = instruction.clone();

    let framed = frame_cpmm_swap_base_input_v2(&mut instruction, &input, &output).unwrap();

    assert_eq!(framed.abi, FramedAbi::CpmmSwapBaseInputV2);
    assert_eq!(
        (framed.input_hook_accounts, framed.output_hook_accounts),
        (3, 4)
    );
    assert_eq!(framed.input_range, 13..16);
    assert_eq!(framed.output_range, 16..20);
    assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR);
    assert_eq!(instruction.data[8..24], before.data[8..24]);
    assert_eq!(&instruction.data[24..], &[3, 0, 4, 0]);
    // Fixed accounts and their flags are untouched; slices are verbatim.
    assert_eq!(&instruction.accounts[..13], &before.accounts[..]);
    assert_eq!(
        &instruction.accounts[13..16],
        input.slice().unwrap().metas()
    );
    assert_eq!(&instruction.accounts[16..], output.slice().unwrap().metas());
    assert_eq!(instruction.program_id, before.program_id);
}

#[tokio::test]
async fn cpmm_frames_only_the_hooked_leg() {
    let fixture = cpmm(None, Some(&[]));
    let (input, output) = fixture.legs().await;
    assert!(!input.is_hooked());
    let mut instruction = fixture.instruction();
    let framed = frame_cpmm_swap_base_input_v2(&mut instruction, &input, &output).unwrap();
    assert_eq!(
        (framed.input_hook_accounts, framed.output_hook_accounts),
        (0, 2)
    );
    assert_eq!(framed.input_range, 13..13);
    assert_eq!(framed.output_range, 13..15);
    assert_eq!(&instruction.data[24..], &[0, 0, 2, 0]);
    assert_eq!(instruction.accounts.len(), 15);
}

#[tokio::test]
async fn cpmm_never_merges_or_dedups_slices_that_share_accounts() {
    let shared = Pubkey::new_unique();
    let fixture = cpmm(Some(&[extra(&shared)]), Some(&[extra(&shared)]));
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction();
    frame_cpmm_swap_base_input_v2(&mut instruction, &input, &output).unwrap();
    assert_eq!(instruction.accounts.len(), 13 + 3 + 3);
    assert_eq!(instruction.accounts[13], ro(shared));
    assert_eq!(instruction.accounts[16], ro(shared));
    assert_eq!(&instruction.data[24..], &[3, 0, 3, 0]);
}

#[tokio::test]
async fn cpmm_passthrough_keeps_unhooked_swaps_byte_identical_v1() {
    let fixture = cpmm(None, None);
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction();
    let before = instruction.clone();
    assert_eq!(
        frame_cpmm_or_passthrough(&mut instruction, &input, &output),
        Ok(None)
    );
    assert_eq!(instruction, before);
    assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR);

    // The explicit framer still produces an all-zero-count V2 when asked.
    let framed = frame_cpmm_swap_base_input_v2(&mut instruction, &input, &output).unwrap();
    assert_eq!(
        (framed.input_hook_accounts, framed.output_hook_accounts),
        (0, 0)
    );
    assert_eq!(&instruction.data[24..], &[0, 0, 0, 0]);
    assert_eq!(instruction.accounts.len(), 13);
}

#[tokio::test]
async fn cpmm_passthrough_frames_when_any_leg_is_hooked() {
    let fixture = cpmm(Some(&[]), None);
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction();
    let framed = frame_cpmm_or_passthrough(&mut instruction, &input, &output)
        .unwrap()
        .expect("hooked leg must frame");
    assert_eq!(framed.input_hook_accounts, 2);
    assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR);
}

#[tokio::test]
async fn cpmm_rejects_bad_instructions_without_touching_them() {
    let fixture = cpmm(Some(&[]), Some(&[]));
    let (input, output) = fixture.legs().await;

    let mut framed_once = fixture.instruction();
    frame_cpmm_swap_base_input_v2(&mut framed_once, &input, &output).unwrap();
    let snapshot = framed_once.clone();
    assert_eq!(
        frame_cpmm_swap_base_input_v2(&mut framed_once, &input, &output),
        Err(FrameError::AlreadyFramed)
    );
    assert_eq!(framed_once, snapshot);

    let mut wrong_data = fixture.instruction();
    wrong_data.data.pop();
    let snapshot = wrong_data.clone();
    assert_eq!(
        frame_cpmm_swap_base_input_v2(&mut wrong_data, &input, &output),
        Err(FrameError::InvalidInstructionData)
    );
    assert_eq!(wrong_data, snapshot);

    let mut wrong_discriminator = fixture.instruction();
    wrong_discriminator.data[..8].copy_from_slice(&CLMM_SWAP_V2_DISCRIMINATOR);
    assert_eq!(
        frame_cpmm_or_passthrough(&mut wrong_discriminator, &input, &output),
        Err(FrameError::InvalidInstructionData)
    );

    let mut short = fixture.instruction();
    short.accounts.pop();
    let snapshot = short.clone();
    assert_eq!(
        frame_cpmm_swap_base_input_v2(&mut short, &input, &output),
        Err(FrameError::InvalidFixedAccountCount {
            expected: 13,
            found: 12
        })
    );
    assert_eq!(short, snapshot);
}

#[tokio::test]
async fn cpmm_rejects_legs_that_do_not_belong_to_the_swap() {
    let fixture = cpmm(Some(&[]), Some(&[]));
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction();
    let snapshot = instruction.clone();

    // Legs passed in the wrong positions.
    let error = frame_cpmm_swap_base_input_v2(&mut instruction, &output, &input).unwrap_err();
    assert!(matches!(
        error,
        FrameError::LegMismatch {
            leg: LegRole::Output,
            field: LegField::Mint,
            ..
        }
    ));
    assert_eq!(instruction, snapshot);

    // A leg for the right mint but the wrong accounts.
    let options = ResolveOptions::default();
    for (field, tamper) in [
        (
            LegField::Source,
            SplTransferLeg {
                source: Pubkey::new_unique(),
                ..fixture.input_leg
            },
        ),
        (
            LegField::Destination,
            SplTransferLeg {
                destination: Pubkey::new_unique(),
                ..fixture.input_leg
            },
        ),
        (
            LegField::Authority,
            SplTransferLeg {
                authority: Pubkey::new_unique(),
                ..fixture.input_leg
            },
        ),
    ] {
        let bad = resolve_leg(LegRole::Input, tamper, &options, fixture.chain.fetcher())
            .await
            .unwrap();
        match frame_cpmm_swap_base_input_v2(&mut instruction, &bad, &output).unwrap_err() {
            FrameError::LegMismatch {
                leg: LegRole::Input,
                field: found,
                ..
            } => assert_eq!(found, field),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(instruction, snapshot);
    }
}

#[tokio::test]
async fn cpmm_checks_that_slices_are_authentic() {
    let fixture = cpmm(None, None);
    let mut instruction = fixture.instruction();
    let snapshot = instruction.clone();
    let hook = Pubkey::new_unique();
    let mint = fixture.accounts.input_token_mint;
    let (good_tail, list) = well_formed_tail(&mint, hook);
    let output = LegHook::new(LegRole::Output, fixture.output_leg, None);
    let input_leg = |metas: Vec<AccountMeta>, hook: Pubkey, list: Pubkey| {
        forged(LegRole::Input, fixture.input_leg, metas, hook, list)
    };

    let cases: Vec<(LegHook, SliceFault)> = vec![
        (input_leg(vec![ro(hook)], hook, list), SliceFault::TooShort),
        (
            input_leg(vec![ro(Pubkey::new_unique()), ro(list)], hook, list),
            SliceFault::TailNotHookProgram,
        ),
        (
            input_leg(vec![ro(hook), ro(Pubkey::new_unique())], hook, list),
            SliceFault::TailNotValidationList,
        ),
        (
            input_leg(vec![AccountMeta::new(hook, false), ro(list)], hook, list),
            SliceFault::TailPrivileged,
        ),
        (
            input_leg(
                vec![ro(hook), AccountMeta::new_readonly(list, true)],
                hook,
                list,
            ),
            SliceFault::TailPrivileged,
        ),
    ];
    for (leg, reason) in cases {
        assert_eq!(
            frame_cpmm_swap_base_input_v2(&mut instruction, &leg, &output),
            Err(FrameError::InvalidSlice {
                leg: LegRole::Input,
                reason
            })
        );
        assert_eq!(instruction, snapshot);
    }

    let wrong_list = Pubkey::new_unique();
    let non_canonical = input_leg(vec![ro(hook), ro(wrong_list)], hook, wrong_list);
    assert_eq!(
        frame_cpmm_swap_base_input_v2(&mut instruction, &non_canonical, &output),
        Err(FrameError::InvalidSlice {
            leg: LegRole::Input,
            reason: SliceFault::NonCanonicalValidationList {
                expected: list,
                found: wrong_list
            }
        })
    );
    assert_eq!(instruction, snapshot);

    // The well-formed forgery passes, proving the checks above are what reject the others.
    let fine = input_leg(good_tail, hook, list);
    assert!(frame_cpmm_swap_base_input_v2(&mut instruction, &fine, &output).is_ok());
}

#[tokio::test]
async fn cpmm_rejects_slices_that_escalate_shared_accounts() {
    let fixture = cpmm(None, None);
    let snapshot = fixture.instruction();
    let hook = Pubkey::new_unique();
    let mint = fixture.accounts.input_token_mint;
    let (_, list) = well_formed_tail(&mint, hook);
    let output = LegHook::new(LegRole::Output, fixture.output_leg, None);

    // Writable extra that is a readonly fixed account (amm_config at index 2).
    let escalated = forged(
        LegRole::Input,
        fixture.input_leg,
        vec![
            AccountMeta::new(fixture.accounts.amm_config, false),
            ro(hook),
            ro(list),
        ],
        hook,
        list,
    );
    let mut instruction = snapshot.clone();
    assert_eq!(
        frame_cpmm_swap_base_input_v2(&mut instruction, &escalated, &output),
        Err(FrameError::CrossSlicePrivilegeConflict {
            leg: LegRole::Input,
            address: fixture.accounts.amm_config,
            other: ConflictSite::Fixed(2)
        })
    );
    assert_eq!(instruction, snapshot);

    // Signer extra that is a non-signer fixed account (the pool vault).
    let signer = forged(
        LegRole::Input,
        fixture.input_leg,
        vec![
            AccountMeta::new_readonly(fixture.accounts.input_vault, true),
            ro(hook),
            ro(list),
        ],
        hook,
        list,
    );
    assert!(matches!(
        frame_cpmm_swap_base_input_v2(&mut instruction, &signer, &output),
        Err(FrameError::CrossSlicePrivilegeConflict {
            other: ConflictSite::Fixed(6),
            ..
        })
    ));

    // The same fixed key with equal-or-lower privilege is fine.
    let lower = forged(
        LegRole::Input,
        fixture.input_leg,
        vec![ro(fixture.accounts.input_vault), ro(hook), ro(list)],
        hook,
        list,
    );
    assert!(frame_cpmm_swap_base_input_v2(&mut instruction, &lower, &output).is_ok());
}

#[tokio::test]
async fn cpmm_rejects_a_key_shared_by_both_slices_with_different_flags() {
    let fixture = cpmm(None, None);
    let shared = Pubkey::new_unique();
    let (hook_a, hook_b) = (Pubkey::new_unique(), Pubkey::new_unique());
    let list_a = get_extra_account_metas_address(&fixture.accounts.input_token_mint, &hook_a);
    let list_b = get_extra_account_metas_address(&fixture.accounts.output_token_mint, &hook_b);
    let input = forged(
        LegRole::Input,
        fixture.input_leg,
        vec![ro(shared), ro(hook_a), ro(list_a)],
        hook_a,
        list_a,
    );
    let output_with = |meta: AccountMeta| {
        forged(
            LegRole::Output,
            fixture.output_leg,
            vec![meta, ro(hook_b), ro(list_b)],
            hook_b,
            list_b,
        )
    };
    let mut instruction = fixture.instruction();
    let snapshot = instruction.clone();
    assert_eq!(
        frame_cpmm_swap_base_input_v2(
            &mut instruction,
            &input,
            &output_with(AccountMeta::new(shared, false))
        ),
        Err(FrameError::CrossSlicePrivilegeConflict {
            leg: LegRole::Output,
            address: shared,
            other: ConflictSite::Leg(LegRole::Input)
        })
    );
    assert_eq!(instruction, snapshot);
    // Identical flags across legs are allowed and both copies are kept.
    assert!(
        frame_cpmm_swap_base_input_v2(&mut instruction, &input, &output_with(ro(shared))).is_ok()
    );
}

#[tokio::test]
async fn cpmm_rejects_slices_longer_than_the_u16_framing_limit() {
    let fixture = cpmm(None, None);
    let hook = Pubkey::new_unique();
    let (tail, list) = well_formed_tail(&fixture.accounts.input_token_mint, hook);
    let mut metas: Vec<AccountMeta> = (0..65_534).map(|_| ro(Pubkey::new_unique())).collect();
    metas.extend(tail);
    assert_eq!(metas.len(), 65_536);
    let huge = forged(LegRole::Input, fixture.input_leg, metas, hook, list);
    let output = LegHook::new(LegRole::Output, fixture.output_leg, None);
    let mut instruction = fixture.instruction();
    let snapshot = instruction.clone();
    assert_eq!(
        frame_cpmm_swap_base_input_v2(&mut instruction, &huge, &output),
        Err(FrameError::AccountCountOverflow)
    );
    assert_eq!(instruction, snapshot);
}

#[tokio::test]
async fn leg_two_failure_leaves_the_v1_instruction_byte_identical_end_to_end() {
    // Resolve both legs, then fail framing on leg two: nothing from leg one may stick.
    let fixture = cpmm(Some(&[]), Some(&[]));
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction();
    let snapshot = instruction.clone();
    let bad_output = LegHook::new(
        LegRole::Output,
        SplTransferLeg {
            destination: Pubkey::new_unique(),
            ..fixture.output_leg
        },
        output.slice().cloned(),
    );
    assert!(frame_cpmm_swap_base_input_v2(&mut instruction, &input, &bad_output).is_err());
    assert_eq!(instruction, snapshot);
}

// ----- CLMM --------------------------------------------------------------------------

struct Clmm {
    accounts: ClmmSwapAccounts,
    chain: MemoryChain,
    input_leg: SplTransferLeg,
    output_leg: SplTransferLeg,
}

fn clmm(hooked_input: bool, hooked_output: bool) -> Clmm {
    let accounts = clmm_accounts();
    let mut chain = MemoryChain::new();
    for (mint, hooked) in [
        (accounts.input_vault_mint, hooked_input),
        (accounts.output_vault_mint, hooked_output),
    ] {
        if hooked {
            chain.add_hooked_mint(mint, Pubkey::new_unique(), None, &[]);
        } else {
            chain.add_unhooked_token_2022_mint(mint);
        }
    }
    let input_leg = SplTransferLeg {
        source: accounts.input_token_account,
        mint: accounts.input_vault_mint,
        destination: accounts.input_vault,
        authority: accounts.payer,
        amount: 77,
    };
    let output_leg = SplTransferLeg {
        source: accounts.output_vault,
        mint: accounts.output_vault_mint,
        destination: accounts.output_token_account,
        authority: accounts.pool_state,
        amount: 70,
    };
    Clmm {
        accounts,
        chain,
        input_leg,
        output_leg,
    }
}

impl Clmm {
    fn instruction(&self, ticks: usize, bitmap: bool) -> Instruction {
        let tick_keys: Vec<Pubkey> = (0..ticks).map(|_| Pubkey::new_unique()).collect();
        build_clmm_swap_v2(
            PROGRAM,
            &self.accounts,
            &tick_keys,
            bitmap.then(Pubkey::new_unique),
            ClmmSwapArgs {
                amount: 77,
                other_amount_threshold: 1,
                sqrt_price_limit_x64: 0,
                is_base_input: true,
            },
        )
    }

    async fn legs(&self) -> (LegHook, LegHook) {
        let options = ResolveOptions::default();
        (
            resolve_leg(
                LegRole::Input,
                self.input_leg,
                &options,
                self.chain.fetcher(),
            )
            .await
            .unwrap(),
            resolve_leg(
                LegRole::Output,
                self.output_leg,
                &options,
                self.chain.fetcher(),
            )
            .await
            .unwrap(),
        )
    }
}

#[tokio::test]
async fn clmm_frames_v2_to_v3_with_ticks_bitmap_and_two_slices() {
    let fixture = clmm(true, true);
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction(3, true);
    let before = instruction.clone();
    assert_eq!(before.accounts.len(), 13 + 3 + 1);

    let framed = frame_clmm_swap_v3(&mut instruction, 3, 1, &input, &output).unwrap();

    assert_eq!(framed.abi, FramedAbi::ClmmSwapV3);
    assert_eq!(instruction.data[..8], CLMM_SWAP_V3_DISCRIMINATOR);
    assert_eq!(instruction.data[8..41], before.data[8..41]);
    assert_eq!(&instruction.data[41..], &[3, 0, 1, 0, 2, 0, 2, 0]);
    assert_eq!(&instruction.accounts[..17], &before.accounts[..]);
    assert_eq!(framed.input_range, 17..19);
    assert_eq!(framed.output_range, 19..21);
    assert_eq!(
        &instruction.accounts[17..19],
        input.slice().unwrap().metas()
    );
    assert_eq!(
        &instruction.accounts[19..21],
        output.slice().unwrap().metas()
    );
}

#[tokio::test]
async fn clmm_passthrough_leaves_unhooked_swap_v2_untouched() {
    let fixture = clmm(false, false);
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction(2, false);
    let before = instruction.clone();
    assert_eq!(
        frame_clmm_or_passthrough(&mut instruction, 2, 0, &input, &output),
        Ok(None)
    );
    assert_eq!(instruction, before);
    assert_eq!(instruction.data[..8], CLMM_SWAP_V2_DISCRIMINATOR);

    let hooked = clmm(false, true);
    let (input, output) = hooked.legs().await;
    let mut instruction = hooked.instruction(2, false);
    let framed = frame_clmm_or_passthrough(&mut instruction, 2, 0, &input, &output)
        .unwrap()
        .unwrap();
    assert_eq!(
        (framed.input_hook_accounts, framed.output_hook_accounts),
        (0, 2)
    );
}

#[tokio::test]
async fn clmm_rejects_malformed_sections_and_instructions_without_mutation() {
    let fixture = clmm(true, true);
    let (input, output) = fixture.legs().await;
    let mut instruction = fixture.instruction(2, true);
    let snapshot = instruction.clone();

    for (ticks, bitmaps) in [(1, 1), (2, 0), (3, 1), (1, 2), (0, 0), (u16::MAX, u16::MAX)] {
        assert_eq!(
            frame_clmm_swap_v3(&mut instruction, ticks, bitmaps, &input, &output),
            Err(FrameError::InvalidRemainingAccountSections),
            "ticks {ticks} bitmaps {bitmaps}"
        );
        assert_eq!(instruction, snapshot);
    }

    let mut bad_data = snapshot.clone();
    bad_data.data.push(0);
    assert_eq!(
        frame_clmm_swap_v3(&mut bad_data, 2, 1, &input, &output),
        Err(FrameError::InvalidInstructionData)
    );

    let mut too_few = fixture.instruction(0, false);
    too_few.accounts.truncate(12);
    assert_eq!(
        frame_clmm_swap_v3(&mut too_few, 0, 0, &input, &output),
        Err(FrameError::InvalidFixedAccountCount {
            expected: 13,
            found: 12
        })
    );

    let mut framed = snapshot.clone();
    frame_clmm_swap_v3(&mut framed, 2, 1, &input, &output).unwrap();
    assert_eq!(
        frame_clmm_swap_v3(&mut framed, 2, 1, &input, &output),
        Err(FrameError::AlreadyFramed)
    );
}

#[tokio::test]
async fn clmm_output_leg_is_authorized_by_the_pool_state() {
    let fixture = clmm(true, true);
    let (input, _) = fixture.legs().await;
    let options = ResolveOptions::default();
    let wrong_authority = resolve_leg(
        LegRole::Output,
        SplTransferLeg {
            authority: fixture.accounts.payer,
            ..fixture.output_leg
        },
        &options,
        fixture.chain.fetcher(),
    )
    .await
    .unwrap();
    let mut instruction = fixture.instruction(1, false);
    let snapshot = instruction.clone();
    assert_eq!(
        frame_clmm_swap_v3(&mut instruction, 1, 0, &input, &wrong_authority),
        Err(FrameError::LegMismatch {
            leg: LegRole::Output,
            field: LegField::Authority,
            expected: fixture.accounts.pool_state,
            found: fixture.accounts.payer
        })
    );
    assert_eq!(instruction, snapshot);
}
