use crate::*;
use transfer_hook_sdk::solana_program::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::*;
use transfer_hook_sdk::{
    build_clmm_swap_v2,
    spl_tlv_account_resolution::account::ExtraAccountMeta,
    testing::{block_on, MemoryChain},
    ClmmSwapArgs, CLMM_SWAP_V2_DISCRIMINATOR, CLMM_SWAP_V3_DISCRIMINATOR,
};

fn accounts() -> ClmmSwapAccounts {
    ClmmSwapAccounts {
        payer: Pubkey::new_unique(),
        amm_config: Pubkey::new_unique(),
        pool_state: Pubkey::new_unique(),
        input_token_account: Pubkey::new_unique(),
        output_token_account: Pubkey::new_unique(),
        input_vault: Pubkey::new_unique(),
        output_vault: Pubkey::new_unique(),
        observation_state: Pubkey::new_unique(),
        token_program: transfer_hook_sdk::spl_token::id(),
        token_program_2022: transfer_hook_sdk::spl_token_2022::id(),
        memo_program: Pubkey::new_unique(),
        input_vault_mint: Pubkey::new_unique(),
        output_vault_mint: Pubkey::new_unique(),
    }
}

fn swap_v2(accounts: &ClmmSwapAccounts) -> Instruction {
    build_clmm_swap_v2(
        Pubkey::new_unique(),
        accounts,
        &[Pubkey::new_unique(), Pubkey::new_unique()],
        Some(Pubkey::new_unique()),
        ClmmSwapArgs {
            amount: 100,
            other_amount_threshold: 1,
            sqrt_price_limit_x64: 0,
            is_base_input: true,
        },
    )
}

#[test]
fn tick_prefix_and_per_leg_hook_slices_are_framed_separately() {
    let accounts = accounts();
    let extra_in = Pubkey::new_unique();
    let mut chain = MemoryChain::new();
    chain.add_hooked_mint(
        accounts.input_vault_mint,
        Pubkey::new_unique(),
        None,
        &[ExtraAccountMeta::new_with_pubkey(&extra_in, false, false).unwrap()],
    );
    chain.add_hooked_mint(accounts.output_vault_mint, Pubkey::new_unique(), None, &[]);
    let (input, output) = clmm_swap_legs(&accounts, 100, 90);
    let plan = block_on(plan_clmm_swap_v3(
        2,
        1,
        input,
        output,
        &ResolveOptions::default(),
        &ResolveOptions::default(),
        chain.fetcher(),
    ))
    .unwrap();
    assert_eq!(plan.hook_account_count(), 5);

    let mut instruction = swap_v2(&accounts);
    assert_eq!(instruction.data[..8], CLMM_SWAP_V2_DISCRIMINATOR);
    let framed = plan.frame(&mut instruction).unwrap().unwrap();
    assert_eq!(instruction.data[..8], CLMM_SWAP_V3_DISCRIMINATOR);
    assert_eq!(&instruction.data[41..], &[2, 0, 1, 0, 3, 0, 2, 0]);
    assert_eq!(framed.input_range, 16..19);
    assert_eq!(framed.output_range, 19..21);
    block_on(plan.verify_unchanged(chain.fetcher())).unwrap();
}

#[test]
fn unhooked_clmm_swap_stays_swap_v2() {
    let accounts = accounts();
    let mut chain = MemoryChain::new();
    chain.add_unhooked_token_2022_mint(accounts.input_vault_mint);
    chain.add_classic_mint(accounts.output_vault_mint);
    let (input, output) = clmm_swap_legs(&accounts, 100, 90);
    let plan = block_on(plan_clmm_swap_v3(
        2,
        1,
        input,
        output,
        &ResolveOptions::default(),
        &ResolveOptions::default(),
        chain.fetcher(),
    ))
    .unwrap();
    let mut instruction = swap_v2(&accounts);
    let before = instruction.clone();
    assert_eq!(plan.frame(&mut instruction), Ok(None));
    assert_eq!(instruction, before);
}

#[test]
fn a_wrong_tick_count_is_rejected_by_the_framer() {
    let accounts = accounts();
    let mut chain = MemoryChain::new();
    chain.add_hooked_mint(accounts.input_vault_mint, Pubkey::new_unique(), None, &[]);
    chain.add_hooked_mint(accounts.output_vault_mint, Pubkey::new_unique(), None, &[]);
    let (input, output) = clmm_swap_legs(&accounts, 100, 90);
    let plan = block_on(plan_clmm_swap_v3(
        5,
        1,
        input,
        output,
        &ResolveOptions::default(),
        &ResolveOptions::default(),
        chain.fetcher(),
    ))
    .unwrap();
    let mut instruction = swap_v2(&accounts);
    assert_eq!(
        plan.frame(&mut instruction),
        Err(FrameError::InvalidRemainingAccountSections)
    );
}
