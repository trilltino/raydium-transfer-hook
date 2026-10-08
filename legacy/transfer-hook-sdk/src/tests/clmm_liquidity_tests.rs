//! The two-token CLMM operations (positions, liquidity, fee collection) framed into their
//! hook-aware instructions.
//!
//! The expected positions of each leg's accounts are written out here from the fork's `Accounts`
//! structs, independently of the table the framer uses, so a wrong layout fails a test.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::account::ExtraAccountMeta;

use crate::{
    error::{FrameError, LegField, LegRole},
    frame::{
        frame_clmm_liquidity_or_passthrough, frame_clmm_liquidity_v3, ClmmLiquidityOp, FramedAbi,
    },
    resolve::{resolve_leg, LegHook, ResolveOptions, SplTransferLeg},
    testing::MemoryChain,
};

const PROGRAM: Pubkey = Pubkey::new_from_array([9; 32]);

/// `(source, destination, authority, mint)` of the token_0 and token_1 transfers, by position in
/// the fixed account list of the fork's `Accounts` struct.
type Positions = ((usize, usize, usize, usize), (usize, usize, usize, usize));

fn expected_positions(op: ClmmLiquidityOp) -> Positions {
    match op {
        // payer, position_nft_owner, position_nft_mint, position_nft_account, pool_state,
        // protocol_position, tick_array_lower, tick_array_upper, personal_position,
        // token_account_0, token_account_1, token_vault_0, token_vault_1, rent, system_program,
        // token_program, associated_token_program, token_program_2022, vault_0_mint, vault_1_mint
        ClmmLiquidityOp::OpenPositionWithToken22Nft => ((9, 11, 0, 18), (10, 12, 0, 19)),
        // payer, position_nft_owner, position_nft_mint, position_nft_account, metadata_account,
        // pool_state, protocol_position, tick_array_lower, tick_array_upper, personal_position,
        // token_account_0, token_account_1, token_vault_0, token_vault_1, rent, system_program,
        // token_program, associated_token_program, metadata_program, token_program_2022,
        // vault_0_mint, vault_1_mint
        ClmmLiquidityOp::OpenPosition => ((10, 12, 0, 20), (11, 13, 0, 21)),
        // nft_owner, nft_account, pool_state, protocol_position, personal_position,
        // tick_array_lower, tick_array_upper, token_account_0, token_account_1, token_vault_0,
        // token_vault_1, token_program, token_program_2022, vault_0_mint, vault_1_mint
        ClmmLiquidityOp::IncreaseLiquidity => ((7, 9, 0, 13), (8, 10, 0, 14)),
        // nft_owner, nft_account, personal_position, pool_state, protocol_position, token_vault_0,
        // token_vault_1, tick_array_lower, tick_array_upper, recipient_token_account_0,
        // recipient_token_account_1, token_program, token_program_2022, memo_program,
        // vault_0_mint, vault_1_mint; the pool state signs.
        ClmmLiquidityOp::DecreaseLiquidity => ((5, 9, 3, 14), (6, 10, 3, 15)),
        // owner, pool_state, amm_config, token_vault_0, token_vault_1, vault_0_mint, vault_1_mint,
        // recipient_token_account_0, recipient_token_account_1, token_program, token_program_2022
        ClmmLiquidityOp::CollectProtocolFee | ClmmLiquidityOp::CollectFundFee => {
            ((3, 7, 1, 5), (4, 8, 1, 6))
        }
    }
}

fn expected_fixed(op: ClmmLiquidityOp) -> usize {
    match op {
        ClmmLiquidityOp::OpenPositionWithToken22Nft => 20,
        ClmmLiquidityOp::OpenPosition => 22,
        ClmmLiquidityOp::IncreaseLiquidity => 15,
        ClmmLiquidityOp::DecreaseLiquidity => 16,
        ClmmLiquidityOp::CollectProtocolFee | ClmmLiquidityOp::CollectFundFee => 11,
    }
}

/// Argument bytes after the discriminator of an instruction that has no `Option` argument.
fn exact_args(op: ClmmLiquidityOp) -> Option<usize> {
    match op {
        ClmmLiquidityOp::DecreaseLiquidity => Some(32),
        ClmmLiquidityOp::CollectProtocolFee | ClmmLiquidityOp::CollectFundFee => Some(16),
        _ => None,
    }
}

struct Fixture {
    op: ClmmLiquidityOp,
    keys: Vec<Pubkey>,
    chain: MemoryChain,
    legs: [SplTransferLeg; 2],
}

fn extra(key: &Pubkey) -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_pubkey(key, false, false).unwrap()
}

/// `extras` is the hook's extra accounts for token_0 and token_1; `None` means a plain mint. `tail` is
/// how many accounts the original instruction takes after its fixed list (bitmap extension, rewards).
fn fixture(
    op: ClmmLiquidityOp,
    extras_0: Option<&[ExtraAccountMeta]>,
    extras_1: Option<&[ExtraAccountMeta]>,
    tail: usize,
) -> Fixture {
    let fixed = expected_fixed(op);
    let keys: Vec<Pubkey> = (0..fixed + tail).map(|_| Pubkey::new_unique()).collect();
    let (a, b) = expected_positions(op);
    let mut chain = MemoryChain::new();
    let mut legs = Vec::new();
    for ((source, destination, authority, mint), extras) in [(a, extras_0), (b, extras_1)] {
        match extras {
            Some(extras) => {
                chain.add_hooked_mint(keys[mint], Pubkey::new_unique(), None, extras);
            }
            None => chain.add_classic_mint(keys[mint]),
        }
        legs.push(SplTransferLeg {
            source: keys[source],
            mint: keys[mint],
            destination: keys[destination],
            authority: keys[authority],
            amount: 100,
        });
    }
    Fixture {
        op,
        keys,
        chain,
        legs: [legs[0], legs[1]],
    }
}

impl Fixture {
    /// The original instruction: its discriminator and, for the ones with a fixed argument size, zero
    /// arguments; the others get a plausible length (`Option<bool>` is `None`).
    fn instruction(&self) -> Instruction {
        let mut data = self.op.v1_discriminator().to_vec();
        data.resize(8 + exact_args(self.op).unwrap_or(60), 0);
        Instruction {
            program_id: PROGRAM,
            accounts: self
                .keys
                .iter()
                .map(|key| AccountMeta::new_readonly(*key, false))
                .collect(),
            data,
        }
    }

    async fn legs(&self) -> (LegHook, LegHook) {
        let options = ResolveOptions::default();
        let token_0 = resolve_leg(
            LegRole::Token0,
            self.legs[0],
            &options,
            self.chain.fetcher(),
        )
        .await
        .unwrap();
        let token_1 = resolve_leg(
            LegRole::Token1,
            self.legs[1],
            &options,
            self.chain.fetcher(),
        )
        .await
        .unwrap();
        (token_0, token_1)
    }
}

#[test]
fn the_discriminators_are_the_ones_the_fork_pins() {
    // Copied from the fork's `hook_aware_liquidity_and_fee_instructions_keep_their_originals_and_add_two_counts`.
    let pinned: [(ClmmLiquidityOp, [u8; 8], [u8; 8]); 6] = [
        (
            ClmmLiquidityOp::OpenPositionWithToken22Nft,
            [77, 255, 174, 82, 125, 29, 201, 46],
            [56, 135, 245, 13, 111, 36, 199, 77],
        ),
        (
            ClmmLiquidityOp::OpenPosition,
            [77, 184, 74, 214, 112, 86, 241, 199],
            [69, 76, 225, 152, 221, 1, 125, 118],
        ),
        (
            ClmmLiquidityOp::IncreaseLiquidity,
            [133, 29, 89, 223, 69, 238, 176, 10],
            [52, 185, 76, 159, 7, 119, 152, 123],
        ),
        (
            ClmmLiquidityOp::DecreaseLiquidity,
            [58, 127, 188, 62, 79, 82, 196, 96],
            [66, 13, 152, 227, 153, 113, 54, 216],
        ),
        (
            ClmmLiquidityOp::CollectProtocolFee,
            [136, 136, 252, 221, 194, 66, 126, 89],
            [246, 11, 93, 67, 221, 244, 185, 10],
        ),
        (
            ClmmLiquidityOp::CollectFundFee,
            [167, 138, 78, 149, 223, 194, 6, 126],
            [21, 250, 142, 236, 215, 232, 49, 184],
        ),
    ];
    assert_eq!(pinned.len(), ClmmLiquidityOp::ALL.len());
    for (op, original, framed) in pinned {
        assert_eq!(op.v1_discriminator(), original, "{op:?} original");
        assert_eq!(op.framed_discriminator(), framed, "{op:?} framed");
        assert_eq!(op.fixed_accounts(), expected_fixed(op), "{op:?} accounts");
    }
}

#[tokio::test]
async fn every_operation_frames_both_slices_last_after_whatever_it_already_takes() {
    for op in ClmmLiquidityOp::ALL {
        for tail in [0, 1, 4] {
            let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
            let f = fixture(op, Some(&[extra(&a)]), Some(&[extra(&a), extra(&b)]), tail);
            let (token_0, token_1) = f.legs().await;
            let mut instruction = f.instruction();
            let before = instruction.clone();

            let framed = frame_clmm_liquidity_v3(op, &mut instruction, &token_0, &token_1)
                .unwrap_or_else(|e| panic!("{op:?}: {e:?}"));

            let existing = expected_fixed(op) + tail;
            assert_eq!(framed.abi, FramedAbi::ClmmLiquidity(op));
            assert_eq!(
                (framed.input_hook_accounts, framed.output_hook_accounts),
                (3, 4)
            );
            assert_eq!(framed.input_range, existing..existing + 3, "{op:?}");
            assert_eq!(framed.output_range, existing + 3..existing + 7, "{op:?}");
            assert_eq!(instruction.data[..8], op.framed_discriminator());
            let args = before.data.len();
            assert_eq!(
                instruction.data[8..args],
                before.data[8..],
                "{op:?} arguments"
            );
            assert_eq!(&instruction.data[args..], &[3, 0, 4, 0], "{op:?} counts");
            assert_eq!(
                &instruction.accounts[..existing],
                &before.accounts[..],
                "{op:?} keeps its own accounts, in order, tail {tail}"
            );
            assert_eq!(
                &instruction.accounts[existing..existing + 3],
                token_0.slice().unwrap().metas()
            );
            assert_eq!(
                &instruction.accounts[existing + 3..],
                token_1.slice().unwrap().metas()
            );
        }
    }
}

#[tokio::test]
async fn unhooked_operations_stay_byte_identical() {
    for op in ClmmLiquidityOp::ALL {
        let f = fixture(op, None, None, 1);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();
        assert_eq!(
            frame_clmm_liquidity_or_passthrough(op, &mut instruction, &token_0, &token_1),
            Ok(None),
            "{op:?}"
        );
        assert_eq!(instruction, before, "{op:?}");

        // One hooked leg is enough to frame.
        let f = fixture(op, Some(&[]), None, 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let framed = frame_clmm_liquidity_or_passthrough(op, &mut instruction, &token_0, &token_1)
            .unwrap()
            .expect("a hooked leg must frame");
        assert_eq!(
            (framed.input_hook_accounts, framed.output_hook_accounts),
            (2, 0)
        );
    }
}

#[tokio::test]
async fn a_leg_that_is_not_where_the_operation_has_it_is_refused_untouched() {
    for op in ClmmLiquidityOp::ALL {
        let f = fixture(op, Some(&[]), Some(&[]), 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();
        let error = frame_clmm_liquidity_v3(op, &mut instruction, &token_1, &token_0).unwrap_err();
        match error {
            FrameError::LegMismatch { field, .. } => {
                assert!(
                    matches!(field, LegField::Mint | LegField::Source),
                    "{op:?}: {field:?}"
                );
            }
            other => panic!("{op:?}: unexpected {other:?}"),
        }
        assert_eq!(instruction, before, "{op:?}");
    }
}

#[tokio::test]
async fn wrong_instructions_are_refused_without_changes() {
    let op = ClmmLiquidityOp::DecreaseLiquidity;
    let f = fixture(op, Some(&[]), Some(&[]), 0);
    let (token_0, token_1) = f.legs().await;

    // Framed twice.
    let mut once = f.instruction();
    frame_clmm_liquidity_v3(op, &mut once, &token_0, &token_1).unwrap();
    assert_eq!(
        frame_clmm_liquidity_v3(op, &mut once, &token_0, &token_1),
        Err(FrameError::AlreadyFramed)
    );

    // Another instruction's discriminator, and data of the wrong length.
    let mut other = f.instruction();
    other.data[..8].copy_from_slice(&ClmmLiquidityOp::CollectFundFee.v1_discriminator());
    assert_eq!(
        frame_clmm_liquidity_v3(op, &mut other, &token_0, &token_1),
        Err(FrameError::InvalidInstructionData)
    );
    let mut short = f.instruction();
    short.data.pop();
    let before = short.clone();
    assert_eq!(
        frame_clmm_liquidity_v3(op, &mut short, &token_0, &token_1),
        Err(FrameError::InvalidInstructionData)
    );
    assert_eq!(short, before);

    // Too few accounts.
    let mut missing = f.instruction();
    missing.accounts.pop();
    assert_eq!(
        frame_clmm_liquidity_v3(op, &mut missing, &token_0, &token_1),
        Err(FrameError::InvalidFixedAccountCount {
            expected: expected_fixed(op),
            found: expected_fixed(op) - 1,
        })
    );
}

#[tokio::test]
async fn an_instruction_that_ends_in_an_option_may_be_a_byte_longer() {
    for op in [
        ClmmLiquidityOp::OpenPositionWithToken22Nft,
        ClmmLiquidityOp::OpenPosition,
        ClmmLiquidityOp::IncreaseLiquidity,
    ] {
        let f = fixture(op, Some(&[]), Some(&[]), 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        instruction.data.push(0); // `Some(false)` is two bytes where `None` is one
        let args = instruction.data.len();
        frame_clmm_liquidity_v3(op, &mut instruction, &token_0, &token_1).unwrap();
        assert_eq!(instruction.data.len(), args + 4, "{op:?}");
    }
}
