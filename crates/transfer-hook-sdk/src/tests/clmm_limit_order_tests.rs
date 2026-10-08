//! The CLMM limit-order operations framed into their hook-aware instructions.
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
        frame_clmm_limit_order_or_passthrough, frame_clmm_limit_order_v2, ClmmLimitOrderOp,
        FramedAbi,
    },
    resolve::{resolve_leg, LegHook, ResolveOptions, SplTransferLeg},
    testing::MemoryChain,
};

const PROGRAM: Pubkey = Pubkey::new_from_array([9; 32]);

/// `(source, destination, authority, mint)` of the input and output transfers, by position in the
/// fixed account list of the fork's `Accounts` struct; `None` for a token the operation does not move.
type Position = Option<(usize, usize, usize, usize)>;

fn expected_positions(op: ClmmLimitOrderOp) -> (Position, Position) {
    match op {
        // payer, pool_state, tick_array, limit_order_nonce, limit_order, input_token_account,
        // output_token_account, input_vault, output_vault, input_vault_mint, output_vault_mint,
        // input_token_program, system_program
        ClmmLimitOrderOp::Open => (Some((5, 7, 0, 9)), None),
        // owner, pool_state, tick_array, limit_order, input_token_account, input_vault,
        // input_vault_mint, input_token_program
        ClmmLimitOrderOp::Increase => (Some((4, 5, 0, 6)), None),
        // owner, pool_state, tick_array, limit_order, input_token_account, output_token_account,
        // input_vault, output_vault, input_vault_mint, output_vault_mint, token_program,
        // token_program_2022; the pool state signs.
        ClmmLimitOrderOp::Decrease => (Some((6, 4, 1, 8)), Some((7, 5, 1, 9))),
        // signer, pool_state, tick_array, limit_order, output_token_account, output_vault,
        // output_vault_mint, output_token_program; the pool state signs.
        ClmmLimitOrderOp::Settle => (None, Some((5, 4, 1, 6))),
    }
}

fn expected_fixed(op: ClmmLimitOrderOp) -> usize {
    match op {
        ClmmLimitOrderOp::Open => 13,
        ClmmLimitOrderOp::Increase | ClmmLimitOrderOp::Settle => 8,
        ClmmLimitOrderOp::Decrease => 12,
    }
}

fn expected_args(op: ClmmLimitOrderOp) -> usize {
    match op {
        ClmmLimitOrderOp::Open => 14,
        ClmmLimitOrderOp::Increase => 8,
        ClmmLimitOrderOp::Decrease => 16,
        ClmmLimitOrderOp::Settle => 0,
    }
}

struct Fixture {
    op: ClmmLimitOrderOp,
    keys: Vec<Pubkey>,
    chain: MemoryChain,
    legs: [Option<SplTransferLeg>; 2],
}

fn extra(key: &Pubkey) -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_pubkey(key, false, false).unwrap()
}

/// `extras_*` is the hook's extra accounts for the input and output token; `None` is a plain mint. A
/// token the operation does not move gets no leg whatever is passed. `tail` is how many accounts follow
/// the fixed list (the bitmap extension).
fn fixture(
    op: ClmmLimitOrderOp,
    extras_input: Option<&[ExtraAccountMeta]>,
    extras_output: Option<&[ExtraAccountMeta]>,
    tail: usize,
) -> Fixture {
    let keys: Vec<Pubkey> = (0..expected_fixed(op) + tail)
        .map(|_| Pubkey::new_unique())
        .collect();
    let (input, output) = expected_positions(op);
    let mut chain = MemoryChain::new();
    let mut legs = [None, None];
    for (slot, (position, extras)) in [(input, extras_input), (output, extras_output)]
        .into_iter()
        .enumerate()
    {
        let Some((source, destination, authority, mint)) = position else {
            continue;
        };
        match extras {
            Some(extras) => chain.add_hooked_mint(keys[mint], Pubkey::new_unique(), None, extras),
            None => chain.add_classic_mint(keys[mint]),
        };
        legs[slot] = Some(SplTransferLeg {
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
        legs,
    }
}

impl Fixture {
    fn instruction(&self) -> Instruction {
        let mut data = self.op.v1_discriminator().to_vec();
        data.resize(8 + expected_args(self.op), 0);
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

    async fn legs(&self) -> (Option<LegHook>, Option<LegHook>) {
        let options = ResolveOptions::default();
        let mut out = [None, None];
        for (slot, (leg, role)) in self
            .legs
            .iter()
            .zip([LegRole::Input, LegRole::Output])
            .enumerate()
        {
            if let Some(leg) = leg {
                out[slot] = Some(
                    resolve_leg(role, *leg, &options, self.chain.fetcher())
                        .await
                        .unwrap(),
                );
            }
        }
        let [input, output] = out;
        (input, output)
    }
}

#[test]
fn the_discriminators_are_the_ones_the_fork_pins() {
    // Copied from the fork's `hook_aware_liquidity_and_fee_instructions_keep_their_originals_and_add_two_counts`.
    let pinned: [(ClmmLimitOrderOp, [u8; 8], [u8; 8]); 4] = [
        (
            ClmmLimitOrderOp::Open,
            [157, 32, 218, 183, 71, 29, 18, 147],
            [194, 127, 58, 217, 208, 170, 10, 182],
        ),
        (
            ClmmLimitOrderOp::Increase,
            [177, 144, 89, 236, 250, 186, 125, 99],
            [159, 156, 82, 111, 207, 39, 60, 30],
        ),
        (
            ClmmLimitOrderOp::Decrease,
            [117, 157, 60, 103, 66, 49, 163, 0],
            [229, 136, 202, 118, 21, 22, 0, 243],
        ),
        (
            ClmmLimitOrderOp::Settle,
            [205, 78, 116, 33, 92, 105, 26, 96],
            [233, 76, 73, 254, 1, 240, 206, 252],
        ),
    ];
    assert_eq!(pinned.len(), ClmmLimitOrderOp::ALL.len());
    for (op, original, framed) in pinned {
        assert_eq!(op.v1_discriminator(), original, "{op:?} original");
        assert_eq!(op.framed_discriminator(), framed, "{op:?} framed");
        assert_eq!(op.fixed_accounts(), expected_fixed(op), "{op:?} accounts");
    }
}

#[tokio::test]
async fn each_operation_frames_the_slices_of_the_tokens_it_moves_last() {
    for op in ClmmLimitOrderOp::ALL {
        for tail in [0, 1] {
            let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
            let f = fixture(op, Some(&[extra(&a)]), Some(&[extra(&a), extra(&b)]), tail);
            let (input, output) = f.legs().await;
            let mut instruction = f.instruction();
            let before = instruction.clone();

            let framed =
                frame_clmm_limit_order_v2(op, &mut instruction, input.as_ref(), output.as_ref())
                    .unwrap_or_else(|e| panic!("{op:?}: {e:?}"));

            let existing = expected_fixed(op) + tail;
            let (in_count, out_count) = (
                if op.moves_input() { 3 } else { 0 },
                if op.moves_output() { 4 } else { 0 },
            );
            assert_eq!(framed.abi, FramedAbi::ClmmLimitOrder(op));
            assert_eq!(
                (framed.input_hook_accounts, framed.output_hook_accounts),
                (in_count, out_count),
                "{op:?}"
            );
            assert_eq!(framed.input_range, existing..existing + in_count as usize);
            assert_eq!(
                framed.output_range,
                existing + in_count as usize..existing + (in_count + out_count) as usize
            );
            assert_eq!(instruction.data[..8], op.framed_discriminator());
            let args = before.data.len();
            assert_eq!(
                instruction.data[8..args],
                before.data[8..],
                "{op:?} arguments"
            );
            let mut counts = in_count.to_le_bytes().to_vec();
            counts.extend_from_slice(&out_count.to_le_bytes());
            assert_eq!(&instruction.data[args..], &counts[..], "{op:?} counts");
            assert_eq!(
                &instruction.accounts[..existing],
                &before.accounts[..],
                "{op:?} keeps its own accounts, in order"
            );
            assert_eq!(
                instruction.accounts.len(),
                existing + (in_count + out_count) as usize
            );
        }
    }
}

#[tokio::test]
async fn unhooked_operations_stay_byte_identical_and_one_hooked_token_frames() {
    for op in ClmmLimitOrderOp::ALL {
        let f = fixture(op, None, None, 1);
        let (input, output) = f.legs().await;
        let mut instruction = f.instruction();
        let before = instruction.clone();
        assert_eq!(
            frame_clmm_limit_order_or_passthrough(
                op,
                &mut instruction,
                input.as_ref(),
                output.as_ref()
            ),
            Ok(None),
            "{op:?}"
        );
        assert_eq!(instruction, before, "{op:?}");

        // A hooked token that the operation moves is enough to frame.
        let f = fixture(op, Some(&[]), Some(&[]), 0);
        let (input, output) = f.legs().await;
        let mut instruction = f.instruction();
        let framed = frame_clmm_limit_order_or_passthrough(
            op,
            &mut instruction,
            input.as_ref(),
            output.as_ref(),
        )
        .unwrap()
        .expect("a hooked leg must frame");
        assert_eq!(
            (framed.input_hook_accounts, framed.output_hook_accounts),
            (
                u16::from(op.moves_input()) * 2,
                u16::from(op.moves_output()) * 2
            ),
            "{op:?}"
        );
    }
}

#[tokio::test]
async fn a_leg_for_a_token_the_operation_does_not_move_is_refused_and_so_is_a_missing_one() {
    // Settle moves only the output token; give it an input leg taken from a decrease.
    let decrease = fixture(ClmmLimitOrderOp::Decrease, Some(&[]), Some(&[]), 0);
    let (decrease_input, decrease_output) = decrease.legs().await;
    let settle = fixture(ClmmLimitOrderOp::Settle, None, Some(&[]), 0);
    let (_, settle_output) = settle.legs().await;
    let mut instruction = settle.instruction();
    let before = instruction.clone();
    assert_eq!(
        frame_clmm_limit_order_v2(
            ClmmLimitOrderOp::Settle,
            &mut instruction,
            decrease_input.as_ref(),
            settle_output.as_ref()
        ),
        Err(FrameError::UnexpectedLeg {
            leg: LegRole::Input,
            moved: false
        })
    );
    // Open moves only the input token; an output leg is refused, and so is leaving the input out.
    let open = fixture(ClmmLimitOrderOp::Open, Some(&[]), None, 0);
    let (open_input, _) = open.legs().await;
    let mut instruction = open.instruction();
    assert_eq!(
        frame_clmm_limit_order_v2(
            ClmmLimitOrderOp::Open,
            &mut instruction,
            open_input.as_ref(),
            decrease_output.as_ref()
        ),
        Err(FrameError::UnexpectedLeg {
            leg: LegRole::Output,
            moved: false
        })
    );
    assert_eq!(
        frame_clmm_limit_order_v2(ClmmLimitOrderOp::Open, &mut instruction, None, None),
        Err(FrameError::UnexpectedLeg {
            leg: LegRole::Input,
            moved: true
        })
    );
    assert_eq!(instruction, open.instruction());
    let _ = before;
}

#[tokio::test]
async fn legs_that_are_not_where_the_operation_has_them_are_refused_untouched() {
    // A decrease with the input and output swapped.
    let op = ClmmLimitOrderOp::Decrease;
    let f = fixture(op, Some(&[]), Some(&[]), 0);
    let (input, output) = f.legs().await;
    let mut instruction = f.instruction();
    let before = instruction.clone();
    let error = frame_clmm_limit_order_v2(op, &mut instruction, output.as_ref(), input.as_ref())
        .unwrap_err();
    match error {
        FrameError::LegMismatch { field, .. } => {
            assert!(
                matches!(field, LegField::Mint | LegField::Source),
                "{field:?}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(instruction, before);
}

#[tokio::test]
async fn wrong_instructions_are_refused_without_changes() {
    let op = ClmmLimitOrderOp::Decrease;
    let f = fixture(op, Some(&[]), Some(&[]), 0);
    let (input, output) = f.legs().await;

    let mut once = f.instruction();
    frame_clmm_limit_order_v2(op, &mut once, input.as_ref(), output.as_ref()).unwrap();
    assert_eq!(
        frame_clmm_limit_order_v2(op, &mut once, input.as_ref(), output.as_ref()),
        Err(FrameError::AlreadyFramed)
    );

    let mut other = f.instruction();
    other.data[..8].copy_from_slice(&ClmmLimitOrderOp::Settle.v1_discriminator());
    assert_eq!(
        frame_clmm_limit_order_v2(op, &mut other, input.as_ref(), output.as_ref()),
        Err(FrameError::InvalidInstructionData)
    );
    let mut short = f.instruction();
    short.data.pop();
    let before = short.clone();
    assert_eq!(
        frame_clmm_limit_order_v2(op, &mut short, input.as_ref(), output.as_ref()),
        Err(FrameError::InvalidInstructionData)
    );
    assert_eq!(short, before);

    let mut missing = f.instruction();
    missing.accounts.pop();
    assert_eq!(
        frame_clmm_limit_order_v2(op, &mut missing, input.as_ref(), output.as_ref()),
        Err(FrameError::InvalidFixedAccountCount {
            expected: expected_fixed(op),
            found: expected_fixed(op) - 1,
        })
    );
}
