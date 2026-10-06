use crate::curve::calculator::CurveCalculator;
use crate::error::ErrorCode;
use crate::states::*;
use crate::utils::token::*;
use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use std::ops::Range;

#[derive(Accounts)]
pub struct Swap<'info> {
    /// The user performing the swap
    pub payer: Signer<'info>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    /// The factory state to read protocol fees
    #[account(address = pool_state.load()?.amm_config)]
    pub amm_config: Box<Account<'info, AmmConfig>>,

    /// The program account of the pool in which the swap will be performed
    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,

    /// The user token account for input token
    #[account(mut)]
    pub input_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The user token account for output token
    #[account(mut)]
    pub output_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The vault token account for input token
    #[account(
        mut,
        constraint = input_vault.key() == pool_state.load()?.token_0_vault || input_vault.key() == pool_state.load()?.token_1_vault
    )]
    pub input_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The vault token account for output token
    #[account(
        mut,
        constraint = output_vault.key() == pool_state.load()?.token_0_vault || output_vault.key() == pool_state.load()?.token_1_vault
    )]
    pub output_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// SPL program for input token transfers
    pub input_token_program: Interface<'info, TokenInterface>,

    /// SPL program for output token transfers
    pub output_token_program: Interface<'info, TokenInterface>,

    /// The mint of input token
    #[account(
        address = input_vault.mint
    )]
    pub input_token_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The mint of output token
    #[account(
        address = output_vault.mint
    )]
    pub output_token_mint: Box<InterfaceAccount<'info, Mint>>,
    /// The program account for the most recent oracle observation
    #[account(mut, address = pool_state.load()?.observation_key)]
    pub observation_state: AccountLoader<'info, ObservationState>,
}

pub fn swap_base_input<'info>(
    ctx: Context<'info, Swap<'info>>,
    amount_in: u64,
    minimum_amount_out: u64,
) -> Result<()> {
    swap_base_input_inner(ctx, amount_in, minimum_amount_out, &[], &[])
}

pub fn swap_base_input_v2<'info>(
    ctx: Context<'info, Swap<'info>>,
    amount_in: u64,
    minimum_amount_out: u64,
    input_hook_account_count: u16,
    output_hook_account_count: u16,
) -> Result<()> {
    let (input_range, output_range) = hook_account_ranges(
        ctx.remaining_accounts.len(),
        input_hook_account_count,
        output_hook_account_count,
    )?;
    let input_hook_accounts = ctx.remaining_accounts[input_range].to_vec();
    let output_hook_accounts = ctx.remaining_accounts[output_range].to_vec();
    swap_base_input_inner(
        ctx,
        amount_in,
        minimum_amount_out,
        &input_hook_accounts,
        &output_hook_accounts,
    )
}

fn hook_account_ranges(
    remaining_account_count: usize,
    input_count: u16,
    output_count: u16,
) -> Result<(Range<usize>, Range<usize>)> {
    require!(
        (input_count == 0 || input_count >= 2) && (output_count == 0 || output_count >= 2),
        ErrorCode::InvalidHookAccountFraming
    );
    let input_end = usize::from(input_count);
    let output_end = input_end
        .checked_add(usize::from(output_count))
        .ok_or(ErrorCode::InvalidHookAccountFraming)?;
    require_eq!(
        output_end,
        remaining_account_count,
        ErrorCode::InvalidHookAccountFraming
    );
    Ok((0..input_end, input_end..output_end))
}

fn swap_base_input_inner<'info>(
    ctx: Context<'info, Swap<'info>>,
    amount_in: u64,
    minimum_amount_out: u64,
    input_hook_accounts: &[AccountInfo<'info>],
    output_hook_accounts: &[AccountInfo<'info>],
) -> Result<()> {
    let block_timestamp = solana_program::clock::Clock::get()?.unix_timestamp as u64;
    let pool_id = ctx.accounts.pool_state.key();
    let pool_state = &mut ctx.accounts.pool_state.load_mut()?;
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Swap)
        || block_timestamp < pool_state.open_time
    {
        return err!(ErrorCode::NotApproved);
    }

    let transfer_fee =
        get_transfer_fee(&ctx.accounts.input_token_mint.to_account_info(), amount_in)?;
    // Take transfer fees into account for actual amount transferred in
    let actual_amount_in = amount_in.saturating_sub(transfer_fee);
    require_gt!(actual_amount_in, 0);

    let SwapParams {
        trade_direction,
        total_input_token_amount,
        total_output_token_amount,
        token_0_price_x64,
        token_1_price_x64,
        is_creator_fee_on_input,
    } = pool_state.get_swap_params(
        ctx.accounts.input_vault.key(),
        ctx.accounts.output_vault.key(),
        ctx.accounts.input_vault.amount,
        ctx.accounts.output_vault.amount,
    )?;
    let constant_before = u128::from(total_input_token_amount)
        .checked_mul(u128::from(total_output_token_amount))
        .unwrap();

    let creator_fee_rate =
        pool_state.adjust_creator_fee_rate(ctx.accounts.amm_config.creator_fee_rate);
    let result = CurveCalculator::swap_base_input(
        u128::from(actual_amount_in),
        u128::from(total_input_token_amount),
        u128::from(total_output_token_amount),
        ctx.accounts.amm_config.trade_fee_rate,
        creator_fee_rate,
        ctx.accounts.amm_config.protocol_fee_rate,
        ctx.accounts.amm_config.fund_fee_rate,
        is_creator_fee_on_input,
    )
    .ok_or(ErrorCode::ZeroTradingTokens)?;

    let constant_after = u128::from(result.new_input_vault_amount)
        .checked_mul(u128::from(result.new_output_vault_amount))
        .unwrap();
    #[cfg(feature = "enable-log")]
    msg!(
        "input_amount:{}, output_amount:{}, trade_fee:{}, input_transfer_fee:{}, constant_before:{},constant_after:{}, is_creator_fee_on_input:{}, creator_fee:{}",
        result.input_amount,
        result.output_amount,
        result.trade_fee,
        transfer_fee,
        constant_before,
        constant_after,
        is_creator_fee_on_input,
        result.creator_fee,
    );
    require_eq!(
        u64::try_from(result.input_amount).unwrap(),
        actual_amount_in
    );
    let (input_transfer_amount, input_transfer_fee) = (amount_in, transfer_fee);
    let (output_transfer_amount, output_transfer_fee) = {
        let amount_out = u64::try_from(result.output_amount).unwrap();
        let transfer_fee = get_transfer_fee(
            &ctx.accounts.output_token_mint.to_account_info(),
            amount_out,
        )?;
        let amount_received = amount_out.checked_sub(transfer_fee).unwrap();
        require_gt!(amount_received, 0);
        require_gte!(
            amount_received,
            minimum_amount_out,
            ErrorCode::ExceededSlippage
        );
        (amount_out, transfer_fee)
    };

    pool_state.update_fees(
        u64::try_from(result.protocol_fee).unwrap(),
        u64::try_from(result.fund_fee).unwrap(),
        u64::try_from(result.creator_fee).unwrap(),
        trade_direction,
    )?;

    emit!(SwapEvent {
        pool_id,
        input_vault_before: total_input_token_amount,
        output_vault_before: total_output_token_amount,
        input_amount: u64::try_from(result.input_amount).unwrap(),
        output_amount: u64::try_from(result.output_amount).unwrap(),
        input_transfer_fee,
        output_transfer_fee,
        base_input: true,
        input_mint: ctx.accounts.input_token_mint.key(),
        output_mint: ctx.accounts.output_token_mint.key(),
        trade_fee: u64::try_from(result.trade_fee).unwrap(),
        creator_fee: u64::try_from(result.creator_fee).unwrap(),
        creator_fee_on_input: is_creator_fee_on_input,
    });
    require_gte!(constant_after, constant_before);

    transfer_from_user_to_pool_vault_with_hook_accounts(
        ctx.accounts.payer.to_account_info(),
        ctx.accounts.input_token_account.to_account_info(),
        ctx.accounts.input_vault.to_account_info(),
        ctx.accounts.input_token_mint.to_account_info(),
        ctx.accounts.input_token_program.to_account_info(),
        input_transfer_amount,
        ctx.accounts.input_token_mint.decimals,
        input_hook_accounts,
    )?;

    transfer_from_pool_vault_to_user_with_hook_accounts(
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.output_vault.to_account_info(),
        ctx.accounts.output_token_account.to_account_info(),
        ctx.accounts.output_token_mint.to_account_info(),
        ctx.accounts.output_token_program.to_account_info(),
        output_transfer_amount,
        ctx.accounts.output_token_mint.decimals,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_state.auth_bump]]],
        output_hook_accounts,
    )?;

    // update the previous price to the observation
    ctx.accounts.observation_state.load_mut()?.update(
        oracle::block_timestamp(),
        token_0_price_x64,
        token_1_price_x64,
    )?;
    pool_state.recent_epoch = Clock::get()?.epoch;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hook_account_ranges;

    #[test]
    fn hook_account_ranges_keep_transfer_slices_disjoint() {
        let (input, output) = hook_account_ranges(7, 3, 4).unwrap();
        assert_eq!(input, 0..3);
        assert_eq!(output, 3..7);
    }

    #[test]
    fn hook_account_ranges_reject_short_or_unframed_tails() {
        assert!(hook_account_ranges(1, 1, 0).is_err());
        assert!(hook_account_ranges(3, 2, 0).is_err());
        assert!(hook_account_ranges(5, 2, 2).is_err());
    }
}
