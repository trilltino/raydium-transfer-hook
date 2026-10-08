//! Assembling a hooked swap: the AMM's fixed accounts and arguments, then each transfer leg's
//! hook slice, framed so the hook-aware instruction can tell where each slice starts.
//!
//! Resolving the legs (`transfer_hook_sdk::resolve_leg`) is the caller's job, because it needs a
//! chain to read. Given the resolved legs, these functions are pure, and the end-to-end flows and
//! the CLI use exactly the same ones.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{
    build_clmm_swap_v2, build_cpmm_swap_base_input_v1, build_cpmm_swap_base_output_v1,
    frame_clmm_or_passthrough, frame_cpmm_or_passthrough, frame_cpmm_output_or_passthrough,
    ClmmSwapAccounts, ClmmSwapArgs, FrameError, LegHook,
};

use crate::{
    clmm::{Clmm, ClmmPool, MEMO_PROGRAM_ID},
    cpmm::{Cpmm, CpmmPool},
};

/// A CPMM `swap_base_input`: `amount_in` of whichever token is the input, at least `minimum_out`
/// back. `mint0_in` says whether the pool's first mint is the input. When either leg is hooked the
/// instruction is framed as `swap_base_input_v2`; with neither it stays the byte-identical V1.
#[allow(clippy::too_many_arguments)]
pub fn cpmm_swap_instruction(
    cpmm: &Cpmm,
    pool: &CpmmPool,
    payer: Pubkey,
    mint0_in: bool,
    input_account: Pubkey,
    output_account: Pubkey,
    amount_in: u64,
    minimum_out: u64,
    input: &LegHook,
    output: &LegHook,
) -> Result<Instruction, FrameError> {
    let accounts = cpmm.swap_accounts(payer, pool, mint0_in, input_account, output_account);
    let mut instruction =
        build_cpmm_swap_base_input_v1(cpmm.program_id, &accounts, amount_in, minimum_out);
    frame_cpmm_or_passthrough(&mut instruction, input, output)?;
    Ok(instruction)
}

/// A CPMM `swap_base_output` (exact output): receive exactly `amount_out`, spending at most
/// `max_amount_in`. Framed as `swap_base_output_v2` when either leg is hooked; with neither it stays
/// the byte-identical V1.
#[allow(clippy::too_many_arguments)]
pub fn cpmm_swap_output_instruction(
    cpmm: &Cpmm,
    pool: &CpmmPool,
    payer: Pubkey,
    mint0_in: bool,
    input_account: Pubkey,
    output_account: Pubkey,
    max_amount_in: u64,
    amount_out: u64,
    input: &LegHook,
    output: &LegHook,
) -> Result<Instruction, FrameError> {
    let accounts = cpmm.swap_accounts(payer, pool, mint0_in, input_account, output_account);
    let mut instruction =
        build_cpmm_swap_base_output_v1(cpmm.program_id, &accounts, max_amount_in, amount_out);
    frame_cpmm_output_or_passthrough(&mut instruction, input, output)?;
    Ok(instruction)
}

/// A CLMM `swap` with a base-input amount. `tick_arrays` are the arrays the swap walks, in order;
/// `memo_program` is part of the fixed accounts. Framed as `swap_v3` when either leg is hooked.
#[allow(clippy::too_many_arguments)]
pub fn clmm_swap_instruction(
    clmm: &Clmm,
    pool: &ClmmPool,
    payer: Pubkey,
    mint0_in: bool,
    input_account: Pubkey,
    output_account: Pubkey,
    amount_in: u64,
    minimum_out: u64,
    tick_arrays: &[Pubkey],
    input: &LegHook,
    output: &LegHook,
) -> Result<Instruction, FrameError> {
    let (input_vault, output_vault, input_mint, output_mint) = if mint0_in {
        (pool.vault_0, pool.vault_1, pool.mint_0, pool.mint_1)
    } else {
        (pool.vault_1, pool.vault_0, pool.mint_1, pool.mint_0)
    };
    let accounts = ClmmSwapAccounts {
        payer,
        amm_config: pool.amm_config,
        pool_state: pool.pool_state,
        input_token_account: input_account,
        output_token_account: output_account,
        input_vault,
        output_vault,
        observation_state: pool.observation,
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        memo_program: MEMO_PROGRAM_ID,
        input_vault_mint: input_mint,
        output_vault_mint: output_mint,
    };
    let mut instruction = build_clmm_swap_v2(
        clmm.program_id,
        &accounts,
        tick_arrays,
        None,
        ClmmSwapArgs {
            amount: amount_in,
            other_amount_threshold: minimum_out,
            sqrt_price_limit_x64: 0,
            is_base_input: true,
        },
    );
    frame_clmm_or_passthrough(&mut instruction, tick_arrays.len() as u16, 0, input, output)?;
    Ok(instruction)
}
