//! The CLMM reward instructions framed into their hook-aware versions. As for the other CLMM operations,
//! the expected positions are written out here from the fork's `Accounts` structs, independently of the
//! tables the framers use.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::account::ExtraAccountMeta;

use crate::{
    abi::anchor_instruction_discriminator,
    error::{FrameError, LegRole},
    frame::{
        frame_clmm_decrease_with_rewards_v4, frame_clmm_reward_or_passthrough,
        frame_clmm_reward_v2, ClmmRewardOp, FramedAbi,
    },
    resolve::{resolve_leg, LegHook, ResolveOptions, SplTransferLeg},
    testing::MemoryChain,
};

const PROGRAM: Pubkey = Pubkey::new_from_array([9; 32]);

fn extra(key: &Pubkey) -> ExtraAccountMeta {
    ExtraAccountMeta::new_with_pubkey(key, false, false).unwrap()
}

async fn leg(chain: &MemoryChain, role: LegRole, leg: SplTransferLeg) -> LegHook {
    resolve_leg(role, leg, &ResolveOptions::default(), chain.fetcher())
        .await
        .unwrap()
}

/// `(source, destination, authority, mint)` and the number of accounts of each single-leg operation.
fn positions(op: ClmmRewardOp) -> ((usize, usize, usize, usize), usize) {
    match op {
        // reward_funder, funder_token_account, amm_config, pool_state, operation_state,
        // reward_token_mint, reward_token_vault, reward_token_program, system_program, rent
        ClmmRewardOp::InitializeReward => ((1, 6, 0, 5), 10),
        // authority, amm_config, pool_state, operation_state, token_program, token_program_2022,
        // and the remaining reward_token_vault, authority_token_account, reward_vault_mint
        ClmmRewardOp::SetRewardParams => ((7, 6, 0, 8), 9),
        // reward_funder, funder_token_account, pool_state, reward_token_vault, reward_vault_mint,
        // token_program, token_program_2022, memo_program
        ClmmRewardOp::CollectRemainingRewards => ((3, 1, 2, 4), 8),
    }
}

fn args(op: ClmmRewardOp) -> usize {
    match op {
        ClmmRewardOp::InitializeReward => 32,
        ClmmRewardOp::SetRewardParams => 33,
        ClmmRewardOp::CollectRemainingRewards => 1,
    }
}

fn instruction(op: ClmmRewardOp, keys: &[Pubkey]) -> Instruction {
    let mut data = op.v1_discriminator().to_vec();
    data.resize(8 + args(op), 0);
    Instruction {
        program_id: PROGRAM,
        accounts: keys
            .iter()
            .map(|key| AccountMeta::new_readonly(*key, false))
            .collect(),
        data,
    }
}

#[test]
fn the_discriminators_are_the_ones_the_fork_pins() {
    // Copied from the fork's `hook_aware_liquidity_and_fee_instructions_keep_their_originals_and_add_two_counts`.
    let pinned: [(ClmmRewardOp, [u8; 8], [u8; 8]); 3] = [
        (
            ClmmRewardOp::InitializeReward,
            [95, 135, 192, 196, 242, 129, 230, 68],
            [91, 1, 77, 50, 235, 229, 133, 49],
        ),
        (
            ClmmRewardOp::SetRewardParams,
            [112, 52, 167, 75, 32, 201, 211, 137],
            [188, 21, 4, 15, 223, 145, 7, 194],
        ),
        (
            ClmmRewardOp::CollectRemainingRewards,
            [18, 237, 166, 197, 34, 16, 213, 144],
            [194, 24, 63, 178, 217, 208, 73, 63],
        ),
    ];
    for (op, original, framed) in pinned {
        assert_eq!(op.v1_discriminator(), original, "{op:?} original");
        assert_eq!(op.framed_discriminator(), framed, "{op:?} framed");
        assert_eq!(op.fixed_accounts(), positions(op).1, "{op:?} accounts");
    }
    assert_eq!(
        anchor_instruction_discriminator("decrease_liquidity_v4"),
        [226, 126, 121, 44, 246, 51, 45, 43]
    );
}

#[tokio::test]
async fn each_reward_operation_appends_its_slice_and_count_last() {
    for op in ClmmRewardOp::ALL {
        let ((source, destination, authority, mint), accounts) = positions(op);
        let keys: Vec<Pubkey> = (0..accounts + 2).map(|_| Pubkey::new_unique()).collect();
        let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(
            keys[mint],
            Pubkey::new_unique(),
            None,
            &[extra(&a), extra(&b)],
        );
        let reward = leg(
            &chain,
            LegRole::Other(0),
            SplTransferLeg {
                source: keys[source],
                mint: keys[mint],
                destination: keys[destination],
                authority: keys[authority],
                amount: 1,
            },
        )
        .await;
        let mut framed_ix = instruction(op, &keys);
        let before = framed_ix.clone();
        let framed = frame_clmm_reward_v2(op, &mut framed_ix, &reward).unwrap();
        assert_eq!(framed.abi, FramedAbi::ClmmReward(op));
        assert_eq!(framed.input_hook_accounts, 4, "{op:?}");
        assert_eq!(framed_ix.data[..8], op.framed_discriminator());
        assert_eq!(framed_ix.data[8..before.data.len()], before.data[8..]);
        assert_eq!(&framed_ix.data[before.data.len()..], &[4, 0], "{op:?}");
        assert_eq!(&framed_ix.accounts[..keys.len()], &before.accounts[..]);
        assert_eq!(
            &framed_ix.accounts[keys.len()..],
            reward.slice().unwrap().metas()
        );
        assert_eq!(framed.input_range, keys.len()..keys.len() + 4);

        // The same framed twice, and a wrong-length instruction, are refused.
        assert_eq!(
            frame_clmm_reward_v2(op, &mut framed_ix, &reward),
            Err(FrameError::AlreadyFramed)
        );
        let mut short = instruction(op, &keys);
        short.data.pop();
        assert_eq!(
            frame_clmm_reward_v2(op, &mut short, &reward),
            Err(FrameError::InvalidInstructionData)
        );
    }
}

#[tokio::test]
async fn an_unhooked_reward_mint_passes_through_untouched() {
    for op in ClmmRewardOp::ALL {
        let ((source, destination, authority, mint), accounts) = positions(op);
        let keys: Vec<Pubkey> = (0..accounts).map(|_| Pubkey::new_unique()).collect();
        let mut chain = MemoryChain::new();
        chain.add_classic_mint(keys[mint]);
        let reward = leg(
            &chain,
            LegRole::Other(0),
            SplTransferLeg {
                source: keys[source],
                mint: keys[mint],
                destination: keys[destination],
                authority: keys[authority],
                amount: 1,
            },
        )
        .await;
        let mut ix = instruction(op, &keys);
        let before = ix.clone();
        assert_eq!(
            frame_clmm_reward_or_passthrough(op, &mut ix, &reward),
            Ok(None)
        );
        assert_eq!(ix, before);
    }
}

/// The 16-account fixed list of `decrease_liquidity_v2`, then `rewards` groups; returns the instruction,
/// the keys, and the legs for token 0, token 1 and each reward.
async fn decrease(
    rewards: &[Option<&[ExtraAccountMeta]>],
    bitmap: bool,
    extras_0: Option<&[ExtraAccountMeta]>,
) -> (Instruction, Vec<LegHook>, usize) {
    let groups_start = 16 + usize::from(bitmap);
    let keys: Vec<Pubkey> = (0..groups_start + 3 * rewards.len())
        .map(|_| Pubkey::new_unique())
        .collect();
    let mut chain = MemoryChain::new();
    let mut legs = Vec::new();
    // token 0: vault 5 -> recipient 9, pool state (3) signs, mint 14; token 1: 6 -> 10, mint 15.
    for ((source, destination, mint), extras) in [((5, 9, 14), extras_0), ((6, 10, 15), None)] {
        match extras {
            Some(extras) => chain.add_hooked_mint(keys[mint], Pubkey::new_unique(), None, extras),
            None => chain.add_classic_mint(keys[mint]),
        };
        legs.push(SplTransferLeg {
            source: keys[source],
            mint: keys[mint],
            destination: keys[destination],
            authority: keys[3],
            amount: 1,
        });
    }
    for (index, extras) in rewards.iter().enumerate() {
        let group = groups_start + 3 * index;
        match extras {
            Some(extras) => {
                chain.add_hooked_mint(keys[group + 2], Pubkey::new_unique(), None, extras)
            }
            None => chain.add_classic_mint(keys[group + 2]),
        };
        legs.push(SplTransferLeg {
            source: keys[group],
            mint: keys[group + 2],
            destination: keys[group + 1],
            authority: keys[3],
            amount: 1,
        });
    }
    let mut resolved = Vec::new();
    for (index, transfer) in legs.into_iter().enumerate() {
        let role = match index {
            0 => LegRole::Token0,
            1 => LegRole::Token1,
            other => LegRole::Other(other as u8 - 2),
        };
        resolved.push(leg(&chain, role, transfer).await);
    }
    let mut data = ClmmRewardOpName::DECREASE.to_vec();
    data.resize(8 + 32, 0);
    (
        Instruction {
            program_id: PROGRAM,
            accounts: keys
                .iter()
                .map(|key| AccountMeta::new_readonly(*key, false))
                .collect(),
            data,
        },
        resolved,
        groups_start,
    )
}

struct ClmmRewardOpName;
impl ClmmRewardOpName {
    const DECREASE: [u8; 8] = [58, 127, 188, 62, 79, 82, 196, 96];
}

#[tokio::test]
async fn a_decrease_frames_the_pool_tokens_then_each_reward_in_order() {
    let a = Pubkey::new_unique();
    let hooked: &[ExtraAccountMeta] =
        &[ExtraAccountMeta::new_with_pubkey(&a, false, false).unwrap()];
    for bitmap in [false, true] {
        // reward 0 hooked, reward 1 plain, reward 2 hooked; token 0 hooked.
        let (mut ix, legs, start) =
            decrease(&[Some(hooked), None, Some(hooked)], bitmap, Some(hooked)).await;
        let before = ix.clone();
        let refs: Vec<&LegHook> = legs[2..].iter().collect();
        let framed =
            frame_clmm_decrease_with_rewards_v4(&mut ix, &legs[0], &legs[1], &refs, start).unwrap();
        assert_eq!(framed.framed.abi, FramedAbi::ClmmDecreaseWithRewards);
        assert_eq!(framed.framed.input_hook_accounts, 3);
        assert_eq!(framed.framed.output_hook_accounts, 0);
        assert_eq!(framed.reward_hook_accounts, [3, 0, 3]);
        assert_eq!(
            &ix.data[..8],
            &anchor_instruction_discriminator("decrease_liquidity_v4")
        );
        assert_eq!(&ix.data[8..before.data.len()], &before.data[8..]);
        assert_eq!(
            &ix.data[before.data.len()..],
            &[3, 0, 0, 0, 3, 0, 0, 0, 3, 0]
        );
        // The slices follow the instruction's own accounts: token 0, (token 1 none), reward 0, reward 2.
        let own = before.accounts.len();
        assert_eq!(framed.framed.input_range, own..own + 3);
        assert_eq!(framed.reward_ranges[0], own + 3..own + 6);
        assert_eq!(framed.reward_ranges[1], own + 6..own + 6);
        assert_eq!(framed.reward_ranges[2], own + 6..own + 9);
        assert_eq!(ix.accounts.len(), own + 9);
        assert_eq!(&ix.accounts[..own], &before.accounts[..]);
    }
}

#[tokio::test]
async fn a_decrease_without_rewards_still_frames_and_a_wrong_group_count_is_refused() {
    let (mut ix, legs, start) = decrease(&[], false, None).await;
    let framed =
        frame_clmm_decrease_with_rewards_v4(&mut ix, &legs[0], &legs[1], &[], start).unwrap();
    assert_eq!(framed.reward_hook_accounts, [0, 0, 0]);
    assert_eq!(&ix.data[ix.data.len() - 10..], &[0; 10]);

    // One reward leg for an instruction that has two groups.
    let (mut ix, legs, start) = decrease(&[None, None], false, None).await;
    let before = ix.clone();
    let one: Vec<&LegHook> = legs[2..3].iter().collect();
    assert_eq!(
        frame_clmm_decrease_with_rewards_v4(&mut ix, &legs[0], &legs[1], &one, start),
        Err(FrameError::InvalidRemainingAccountSections)
    );
    assert_eq!(ix, before);

    // A reward leg that is not at its group.
    let (mut ix, legs, start) = decrease(&[None, None], false, None).await;
    let swapped: Vec<&LegHook> = vec![&legs[3], &legs[2]];
    assert!(matches!(
        frame_clmm_decrease_with_rewards_v4(&mut ix, &legs[0], &legs[1], &swapped, start),
        Err(FrameError::LegMismatch { .. })
    ));
}
