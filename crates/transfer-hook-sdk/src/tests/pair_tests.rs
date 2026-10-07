//! The two-token CPMM operations (liquidity, fee collection, pool creation) framed into `_v2`.
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
    frame::{frame_cpmm_pair_or_passthrough, frame_cpmm_pair_v2, CpmmPairOp, FramedAbi},
    resolve::{resolve_leg, LegHook, ResolveOptions, SplTransferLeg},
    testing::MemoryChain,
};

const PROGRAM: Pubkey = Pubkey::new_from_array([7; 32]);

/// `(source, destination, authority, mint)` of the token_0 and token_1 transfers, by position in
/// the fixed account list of the fork's `Accounts` struct.
type Positions = ((usize, usize, usize, usize), (usize, usize, usize, usize));

fn expected_positions(op: CpmmPairOp) -> Positions {
    match op {
        // owner, authority, pool_state, owner_lp_token, token_0_account, token_1_account,
        // token_0_vault, token_1_vault, token_program, token_program_2022, vault_0_mint,
        // vault_1_mint, lp_mint
        CpmmPairOp::Deposit => ((4, 6, 0, 10), (5, 7, 0, 11)),
        // the same, then memo_program; the pool authority pays.
        CpmmPairOp::Withdraw => ((6, 4, 1, 10), (7, 5, 1, 11)),
        // owner, authority, pool_state, amm_config, token_0_vault, token_1_vault, vault_0_mint,
        // vault_1_mint, recipient_token_0_account, recipient_token_1_account, token_program,
        // token_program_2022
        CpmmPairOp::CollectProtocolFee | CpmmPairOp::CollectFundFee => ((4, 8, 1, 6), (5, 9, 1, 7)),
        // creator, authority, pool_state, amm_config, token_0_vault, token_1_vault, vault_0_mint,
        // vault_1_mint, creator_token_0, creator_token_1, token_0_program, token_1_program,
        // associated_token_program, system_program, creator_fee_share
        CpmmPairOp::CollectCreatorFee => ((4, 8, 1, 6), (5, 9, 1, 7)),
        // payer, creator, authority, pool_state, token_0_vault, ... (authority is third)
        CpmmPairOp::CollectCreatorFeePermissionless => ((4, 8, 2, 6), (5, 9, 2, 7)),
        // creator, amm_config, authority, pool_state, token_0_mint, token_1_mint, lp_mint,
        // creator_token_0, creator_token_1, creator_lp_token, token_0_vault, token_1_vault, ...
        CpmmPairOp::Initialize => ((7, 10, 0, 4), (8, 11, 0, 5)),
        // payer, creator, amm_config, authority, pool_state, token_0_mint, token_1_mint, lp_mint,
        // payer_token_0, payer_token_1, payer_lp_token, token_0_vault, token_1_vault, ...
        CpmmPairOp::InitializeWithPermission => ((8, 11, 0, 5), (9, 12, 0, 6)),
    }
}

fn expected_fixed(op: CpmmPairOp) -> usize {
    match op {
        CpmmPairOp::Deposit => 13,
        CpmmPairOp::Withdraw => 14,
        CpmmPairOp::CollectProtocolFee | CpmmPairOp::CollectFundFee => 12,
        CpmmPairOp::CollectCreatorFee => 15,
        CpmmPairOp::CollectCreatorFeePermissionless => 16,
        CpmmPairOp::Initialize => 20,
        CpmmPairOp::InitializeWithPermission => 21,
    }
}

struct Fixture {
    op: CpmmPairOp,
    keys: Vec<Pubkey>,
    chain: MemoryChain,
    legs: [SplTransferLeg; 2],
}

fn extra(key: &Pubkey) -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_pubkey(key, false, false).unwrap()
}

/// `extras` is the hook's extra accounts for token_0 and token_1; `None` means a plain mint.
fn fixture(
    op: CpmmPairOp,
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
    fn instruction(&self) -> Instruction {
        let mut data = self.op.v1_discriminator().to_vec();
        data.resize(self.op.v1_data_len(), 0);
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
    // Copied from the fork's `two_token_operations_keep_their_v1_discriminators_and_add_v2`.
    let pinned: [(CpmmPairOp, [u8; 8], [u8; 8]); 8] = [
        (
            CpmmPairOp::Deposit,
            [242, 35, 198, 137, 82, 225, 242, 182],
            [109, 75, 69, 153, 172, 218, 146, 19],
        ),
        (
            CpmmPairOp::Withdraw,
            [183, 18, 70, 156, 148, 109, 161, 34],
            [242, 80, 163, 0, 196, 221, 194, 194],
        ),
        (
            CpmmPairOp::CollectProtocolFee,
            [136, 136, 252, 221, 194, 66, 126, 89],
            [246, 11, 93, 67, 221, 244, 185, 10],
        ),
        (
            CpmmPairOp::CollectFundFee,
            [167, 138, 78, 149, 223, 194, 6, 126],
            [21, 250, 142, 236, 215, 232, 49, 184],
        ),
        (
            CpmmPairOp::CollectCreatorFee,
            [20, 22, 86, 123, 198, 28, 219, 132],
            [207, 17, 138, 242, 4, 34, 19, 56],
        ),
        (
            CpmmPairOp::CollectCreatorFeePermissionless,
            [202, 202, 34, 83, 226, 122, 145, 229],
            [100, 50, 213, 79, 188, 138, 6, 207],
        ),
        (
            CpmmPairOp::Initialize,
            [175, 175, 109, 31, 13, 152, 155, 237],
            [67, 153, 175, 39, 218, 16, 38, 32],
        ),
        (
            CpmmPairOp::InitializeWithPermission,
            [63, 55, 254, 65, 49, 178, 89, 121],
            [20, 6, 23, 116, 191, 226, 176, 71],
        ),
    ];
    assert_eq!(pinned.len(), CpmmPairOp::ALL.len());
    for (op, v1, v2) in pinned {
        assert_eq!(op.v1_discriminator(), v1, "{op:?} v1");
        assert_eq!(op.v2_discriminator(), v2, "{op:?} v2");
        assert_eq!(op.fixed_accounts(), expected_fixed(op), "{op:?} accounts");
    }
}

#[tokio::test]
async fn every_operation_frames_both_slices_in_order_after_its_fixed_accounts() {
    for op in CpmmPairOp::ALL {
        let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
        let f = fixture(op, Some(&[extra(&a)]), Some(&[extra(&a), extra(&b)]), 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();

        let framed = frame_cpmm_pair_v2(op, &mut instruction, &token_0, &token_1)
            .unwrap_or_else(|e| panic!("{op:?}: {e:?}"));

        let fixed = expected_fixed(op);
        assert_eq!(framed.abi, FramedAbi::CpmmPair(op));
        assert_eq!(
            (framed.input_hook_accounts, framed.output_hook_accounts),
            (3, 4)
        );
        assert_eq!(framed.input_range, fixed..fixed + 3, "{op:?}");
        assert_eq!(framed.output_range, fixed + 3..fixed + 7, "{op:?}");
        assert_eq!(instruction.data[..8], op.v2_discriminator());
        let args = before.data.len();
        assert_eq!(
            instruction.data[8..args],
            before.data[8..],
            "{op:?} arguments"
        );
        assert_eq!(&instruction.data[args..], &[3, 0, 4, 0], "{op:?} counts");
        assert_eq!(
            &instruction.accounts[..fixed],
            &before.accounts[..],
            "{op:?}"
        );
        assert_eq!(
            &instruction.accounts[fixed..fixed + 3],
            token_0.slice().unwrap().metas()
        );
        assert_eq!(
            &instruction.accounts[fixed + 3..],
            token_1.slice().unwrap().metas()
        );
    }
}

#[tokio::test]
async fn pool_creation_keeps_its_support_mint_records_after_the_slices() {
    for op in [CpmmPairOp::Initialize, CpmmPairOp::InitializeWithPermission] {
        let a = Pubkey::new_unique();
        let f = fixture(op, Some(&[extra(&a)]), None, 2);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();
        let fixed = expected_fixed(op);

        let framed = frame_cpmm_pair_v2(op, &mut instruction, &token_0, &token_1).unwrap();

        assert_eq!(framed.input_range, fixed..fixed + 3);
        assert_eq!(framed.output_range, fixed + 3..fixed + 3);
        // The two support-mint records moved to the end, in their original order.
        assert_eq!(
            &instruction.accounts[fixed + 3..],
            &before.accounts[fixed..],
            "{op:?}"
        );
        assert_eq!(instruction.accounts.len(), before.accounts.len() + 3);
    }
}

#[tokio::test]
async fn unhooked_operations_stay_byte_identical_v1() {
    for op in CpmmPairOp::ALL {
        let f = fixture(op, None, None, 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();
        assert_eq!(
            frame_cpmm_pair_or_passthrough(op, &mut instruction, &token_0, &token_1),
            Ok(None),
            "{op:?}"
        );
        assert_eq!(instruction, before, "{op:?}");

        // One hooked leg is enough to frame.
        let f = fixture(op, None, Some(&[]), 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let framed = frame_cpmm_pair_or_passthrough(op, &mut instruction, &token_0, &token_1)
            .unwrap()
            .expect("a hooked leg must frame");
        assert_eq!(
            (framed.input_hook_accounts, framed.output_hook_accounts),
            (0, 2)
        );
    }
}

#[tokio::test]
async fn a_leg_that_is_not_where_the_operation_has_it_is_refused_untouched() {
    for op in CpmmPairOp::ALL {
        let f = fixture(op, Some(&[]), Some(&[]), 0);
        let (token_0, token_1) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();
        // token_1's leg offered as token_0's: its mint is not at token_0's position.
        let error = frame_cpmm_pair_v2(op, &mut instruction, &token_1, &token_0).unwrap_err();
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
    let op = CpmmPairOp::Deposit;
    let f = fixture(op, Some(&[]), Some(&[]), 0);
    let (token_0, token_1) = f.legs().await;

    // Framed twice.
    let mut once = f.instruction();
    frame_cpmm_pair_v2(op, &mut once, &token_0, &token_1).unwrap();
    assert_eq!(
        frame_cpmm_pair_v2(op, &mut once, &token_0, &token_1),
        Err(FrameError::AlreadyFramed)
    );

    // A different operation's discriminator.
    let mut other = f.instruction();
    other.data[..8].copy_from_slice(&CpmmPairOp::Withdraw.v1_discriminator());
    let before = other.clone();
    assert_eq!(
        frame_cpmm_pair_v2(op, &mut other, &token_0, &token_1),
        Err(FrameError::InvalidInstructionData)
    );
    assert_eq!(other, before);

    // Wrong data length.
    let mut short = f.instruction();
    short.data.pop();
    assert_eq!(
        frame_cpmm_pair_v2(op, &mut short, &token_0, &token_1),
        Err(FrameError::InvalidInstructionData)
    );

    // A fixed list that is too short, or (for an operation without a tail) too long.
    let mut few = f.instruction();
    few.accounts.pop();
    assert!(matches!(
        frame_cpmm_pair_v2(op, &mut few, &token_0, &token_1),
        Err(FrameError::InvalidFixedAccountCount {
            expected: 13,
            found: 12
        })
    ));
    let mut many = f.instruction();
    many.accounts
        .push(AccountMeta::new_readonly(Pubkey::new_unique(), false));
    assert!(matches!(
        frame_cpmm_pair_v2(op, &mut many, &token_0, &token_1),
        Err(FrameError::InvalidFixedAccountCount {
            expected: 13,
            found: 14
        })
    ));
}
