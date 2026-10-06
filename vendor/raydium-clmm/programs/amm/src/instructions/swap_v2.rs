use crate::error::ErrorCode;
use crate::libraries::tick_math;
use crate::swap::{swap_internal, SwapInternalResult};
use crate::util::*;
use crate::{states::*, util};
use anchor_lang::{prelude::*, solana_program};
use anchor_spl::memo::Memo;
use anchor_spl::token::Token;
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};
use std::{collections::VecDeque, ops::Range};

/// Memo msg for swap
pub const SWAP_MEMO_MSG: &'static [u8] = b"raydium_swap";
#[derive(Accounts)]
pub struct SwapSingleV2<'info> {
    /// The user performing the swap
    pub payer: Signer<'info>,

    /// The factory state to read protocol fees
    #[account(address = pool_state.load()?.amm_config)]
    pub amm_config: Box<Account<'info, AmmConfig>>,

    /// The program account of the pool in which the swap will be performed
    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,

    /// The user token account for input token
    #[account(
        mut,
        token::mint = input_vault.mint,
        token::authority = payer,
    )]
    pub input_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The user token account for output token
    #[account(
        mut,
        token::mint = output_vault.mint,
    )]
    pub output_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The vault token account for input token
    #[account(mut)]
    pub input_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The vault token account for output token
    #[account(mut)]
    pub output_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The program account for the most recent oracle observation
    #[account(mut, address = pool_state.load()?.observation_key)]
    pub observation_state: AccountLoader<'info, ObservationState>,

    /// SPL program for token transfers
    pub token_program: Program<'info, Token>,

    /// SPL program 2022 for token transfers
    pub token_program_2022: Program<'info, Token2022>,

    /// Memo program
    pub memo_program: Program<'info, Memo>,

    /// The mint of token vault 0
    #[account(
        address = input_vault.mint
    )]
    pub input_vault_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The mint of token vault 1
    #[account(
        address = output_vault.mint
    )]
    pub output_vault_mint: Box<InterfaceAccount<'info, Mint>>,
    // remaining accounts
    // tickarray_bitmap_extension: must add account if need
    // tick_array_account_1
    // tick_array_account_2
    // tick_array_account_...
}

/// Performs a single exact input/output swap
/// if is_base_input = true, return value is the max_amount_out, otherwise is min_amount_in
pub fn exact_internal_v2<'info>(
    ctx: &mut SwapSingleV2<'info>,
    remaining_accounts: &'info [AccountInfo<'info>],
    amount_specified: u64,
    sqrt_price_limit_x64: u128,
    is_base_input: bool,
) -> Result<u64> {
    exact_internal_with_hook_accounts(
        ctx,
        remaining_accounts,
        &[],
        &[],
        amount_specified,
        sqrt_price_limit_x64,
        is_base_input,
    )
}

fn exact_internal_with_hook_accounts<'info>(
    ctx: &mut SwapSingleV2<'info>,
    tick_accounts: &'info [AccountInfo<'info>],
    input_hook_accounts: &'info [AccountInfo<'info>],
    output_hook_accounts: &'info [AccountInfo<'info>],
    amount_specified: u64,
    sqrt_price_limit_x64: u128,
    is_base_input: bool,
) -> Result<u64> {
    // invoke_memo_instruction(SWAP_MEMO_MSG, ctx.memo_program.to_account_info())?;

    let block_timestamp = solana_program::clock::Clock::get()?.unix_timestamp as u64;

    let swap_result: SwapInternalResult;
    let zero_for_one;
    let swap_price_before;

    let input_balance_before = ctx.input_token_account.amount;
    let output_balance_before = ctx.output_token_account.amount;

    // calculate specified amount because the amount includes transfer_fee as input and without transfer_fee as output
    let (amount_calculate_specified, transfer_fee) = if is_base_input {
        let transfer_fee = util::get_transfer_fee(ctx.input_vault_mint.clone(), amount_specified)?;
        (amount_specified - transfer_fee, transfer_fee)
    } else {
        let transfer_fee =
            util::get_transfer_inverse_fee(ctx.output_vault_mint.clone(), amount_specified)?;
        (amount_specified + transfer_fee, transfer_fee)
    };

    {
        swap_price_before = ctx.pool_state.load()?.sqrt_price_x64;
        let pool_state = &mut ctx.pool_state.load_mut()?;
        zero_for_one = ctx.input_vault.mint == pool_state.token_mint_0;

        require_gt!(block_timestamp, pool_state.open_time);

        require!(
            if zero_for_one {
                ctx.input_vault.key() == pool_state.token_vault_0
                    && ctx.output_vault.key() == pool_state.token_vault_1
            } else {
                ctx.input_vault.key() == pool_state.token_vault_1
                    && ctx.output_vault.key() == pool_state.token_vault_0
            },
            ErrorCode::InvalidInputPoolVault
        );

        let mut tickarray_bitmap_extension = None;
        let tick_array_states = &mut VecDeque::new();

        for account_info in tick_accounts {
            if account_info.data_len() == TickArrayState::LEN {
                tick_array_states.push_back(AccountLoad::load_data_mut(account_info)?);
            } else if account_info.data_len() == TickArrayBitmapExtension::LEN {
                TickArrayBitmapExtension::validate_belongs_to_pool(account_info, pool_state.key())?;
                tickarray_bitmap_extension = Some(account_info);
            } else {
                break;
            }
        }

        swap_result = swap_internal(
            &ctx.amm_config,
            pool_state,
            tick_array_states,
            &mut ctx.observation_state.load_mut()?,
            tickarray_bitmap_extension,
            amount_calculate_specified,
            if sqrt_price_limit_x64 == 0 {
                if zero_for_one {
                    tick_math::MIN_SQRT_PRICE_X64 + 1
                } else {
                    tick_math::MAX_SQRT_PRICE_X64 - 1
                }
            } else {
                sqrt_price_limit_x64
            },
            zero_for_one,
            is_base_input,
            oracle::block_timestamp(),
        )?;

        #[cfg(feature = "enable-log")]
        msg!(
            "exact_swap_internal, is_base_input:{}, amount_0: {}, amount_1: {}",
            is_base_input,
            swap_result.amount_0,
            swap_result.amount_1
        );
        require!(
            swap_result.amount_0 != 0 && swap_result.amount_1 != 0,
            ErrorCode::TooSmallInputOrOutputAmount
        );
    }
    let (token_account_0, token_account_1, vault_0, vault_1, vault_0_mint, vault_1_mint) =
        if zero_for_one {
            (
                ctx.input_token_account.clone(),
                ctx.output_token_account.clone(),
                ctx.input_vault.clone(),
                ctx.output_vault.clone(),
                ctx.input_vault_mint.clone(),
                ctx.output_vault_mint.clone(),
            )
        } else {
            (
                ctx.output_token_account.clone(),
                ctx.input_token_account.clone(),
                ctx.output_vault.clone(),
                ctx.input_vault.clone(),
                ctx.output_vault_mint.clone(),
                ctx.input_vault_mint.clone(),
            )
        };

    let amount_0_without_fee;
    let amount_1_without_fee;
    let transfer_fee_0;
    let transfer_fee_1;
    let transfer_amount_0;
    let transfer_amount_1;
    if zero_for_one {
        transfer_fee_0 = if is_base_input && swap_result.amount_0 == amount_calculate_specified {
            transfer_fee
        } else {
            util::get_transfer_inverse_fee(vault_0_mint.clone(), swap_result.amount_0)?
        };
        transfer_fee_1 = util::get_transfer_fee(vault_1_mint.clone(), swap_result.amount_1)?;

        amount_0_without_fee = swap_result.amount_0;
        amount_1_without_fee = swap_result
            .amount_1
            .checked_sub(transfer_fee_1)
            .ok_or(ErrorCode::CalculateOverflow)?;
        (transfer_amount_0, transfer_amount_1) = (
            swap_result
                .amount_0
                .checked_add(transfer_fee_0)
                .ok_or(ErrorCode::CalculateOverflow)?,
            swap_result.amount_1,
        );
    } else {
        transfer_fee_0 = util::get_transfer_fee(vault_0_mint.clone(), swap_result.amount_0)?;
        transfer_fee_1 = if is_base_input && swap_result.amount_1 == amount_calculate_specified {
            transfer_fee
        } else {
            util::get_transfer_inverse_fee(vault_1_mint.clone(), swap_result.amount_1)?
        };

        amount_0_without_fee = swap_result
            .amount_0
            .checked_sub(transfer_fee_0)
            .ok_or(ErrorCode::CalculateOverflow)?;
        amount_1_without_fee = swap_result.amount_1;
        (transfer_amount_0, transfer_amount_1) = (
            swap_result.amount_0,
            swap_result
                .amount_1
                .checked_add(transfer_fee_1)
                .ok_or(ErrorCode::CalculateOverflow)?,
        );
    }
    #[cfg(feature = "enable-log")]
    msg!(
        "amount_0:{}, transfer_fee_0:{}, amount_1:{}, transfer_fee_1:{}",
        swap_result.amount_0,
        transfer_fee_0,
        swap_result.amount_1,
        transfer_fee_1
    );

    emit!(SwapEvent {
        pool_state: ctx.pool_state.key(),
        sender: ctx.payer.key(),
        token_account_0: token_account_0.key(),
        token_account_1: token_account_1.key(),
        amount_0: amount_0_without_fee,
        transfer_fee_0,
        amount_1: amount_1_without_fee,
        transfer_fee_1,
        zero_for_one,
        sqrt_price_x64: swap_result.sqrt_price_x64,
        liquidity: swap_result.liquidity,
        tick: swap_result.tick,
        trade_fee_0: swap_result.trade_fee_0,
        trade_fee_1: swap_result.trade_fee_1,
    });

    if zero_for_one {
        //  x -> y, deposit x token from user to pool vault.
        transfer_from_user_to_pool_vault_with_hook_accounts(
            &ctx.payer,
            &token_account_0.to_account_info(),
            &vault_0.to_account_info(),
            Some(vault_0_mint),
            &ctx.token_program,
            Some(ctx.token_program_2022.to_account_info()),
            transfer_amount_0,
            input_hook_accounts,
        )?;
        // x -> y，transfer y token from pool vault to user.
        transfer_from_pool_vault_to_user_with_hook_accounts(
            &ctx.pool_state,
            &vault_1.to_account_info(),
            &token_account_1.to_account_info(),
            Some(vault_1_mint),
            &ctx.token_program,
            Some(ctx.token_program_2022.to_account_info()),
            transfer_amount_1,
            output_hook_accounts,
        )?;
    } else {
        transfer_from_user_to_pool_vault_with_hook_accounts(
            &ctx.payer,
            &token_account_1.to_account_info(),
            &vault_1.to_account_info(),
            Some(vault_1_mint),
            &ctx.token_program,
            Some(ctx.token_program_2022.to_account_info()),
            transfer_amount_1,
            input_hook_accounts,
        )?;
        transfer_from_pool_vault_to_user_with_hook_accounts(
            &ctx.pool_state,
            &vault_0.to_account_info(),
            &token_account_0.to_account_info(),
            Some(vault_0_mint),
            &ctx.token_program,
            Some(ctx.token_program_2022.to_account_info()),
            transfer_amount_0,
            output_hook_accounts,
        )?;
    }
    ctx.output_token_account.reload()?;
    ctx.input_token_account.reload()?;

    if zero_for_one {
        require_gte!(swap_price_before, swap_result.sqrt_price_x64);
    } else {
        require_gte!(swap_result.sqrt_price_x64, swap_price_before);
    }
    if sqrt_price_limit_x64 == 0 {
        // Does't allow partial filled without specified limit_price.
        if is_base_input {
            if zero_for_one {
                require_eq!(amount_specified, transfer_amount_0);
            } else {
                require_eq!(amount_specified, transfer_amount_1);
            }
        } else {
            if zero_for_one {
                require_eq!(amount_calculate_specified, transfer_amount_1);
            } else {
                require_eq!(amount_calculate_specified, transfer_amount_0);
            }
        }
    }

    let result = if is_base_input {
        ctx.output_token_account
            .amount
            .checked_sub(output_balance_before)
            .ok_or(ErrorCode::CalculateOverflow.into())
    } else {
        input_balance_before
            .checked_sub(ctx.input_token_account.amount)
            .ok_or(ErrorCode::CalculateOverflow.into())
    };
    result
}

fn swap_v3_account_ranges(
    remaining_account_count: usize,
    tick_array_count: u16,
    bitmap_count: u16,
    input_hook_account_count: u16,
    output_hook_account_count: u16,
) -> Result<(Range<usize>, Range<usize>, Range<usize>, Range<usize>)> {
    require!(
        bitmap_count <= 1
            && (input_hook_account_count == 0 || input_hook_account_count >= 2)
            && (output_hook_account_count == 0 || output_hook_account_count >= 2),
        ErrorCode::InvalidHookAccountFraming
    );
    let tick_end = usize::from(tick_array_count);
    let bitmap_end = tick_end
        .checked_add(usize::from(bitmap_count))
        .ok_or(ErrorCode::InvalidHookAccountFraming)?;
    let input_end = bitmap_end
        .checked_add(usize::from(input_hook_account_count))
        .ok_or(ErrorCode::InvalidHookAccountFraming)?;
    let output_end = input_end
        .checked_add(usize::from(output_hook_account_count))
        .ok_or(ErrorCode::InvalidHookAccountFraming)?;
    require_eq!(
        output_end,
        remaining_account_count,
        ErrorCode::InvalidHookAccountFraming
    );
    Ok((
        0..tick_end,
        tick_end..bitmap_end,
        bitmap_end..input_end,
        input_end..output_end,
    ))
}

pub fn swap_v2<'info>(
    ctx: Context<'info, SwapSingleV2<'info>>,
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit_x64: u128,
    is_base_input: bool,
) -> Result<()> {
    let amount_result = exact_internal_v2(
        ctx.accounts,
        ctx.remaining_accounts,
        amount,
        sqrt_price_limit_x64,
        is_base_input,
    )?;
    if is_base_input {
        require_gte!(
            amount_result,
            other_amount_threshold,
            ErrorCode::TooLittleOutputReceived
        );
    } else {
        require_gte!(
            other_amount_threshold,
            amount_result,
            ErrorCode::TooMuchInputPaid
        );
    }

    Ok(())
}

pub fn swap_v3<'info>(
    ctx: Context<'info, SwapSingleV2<'info>>,
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit_x64: u128,
    is_base_input: bool,
    tick_array_count: u16,
    bitmap_count: u16,
    input_hook_account_count: u16,
    output_hook_account_count: u16,
) -> Result<()> {
    let (tick_range, bitmap_range, input_range, output_range) = swap_v3_account_ranges(
        ctx.remaining_accounts.len(),
        tick_array_count,
        bitmap_count,
        input_hook_account_count,
        output_hook_account_count,
    )?;
    let remaining_accounts = ctx.remaining_accounts;
    for account in &remaining_accounts[tick_range.clone()] {
        require_eq!(
            account.data_len(),
            TickArrayState::LEN,
            ErrorCode::InvalidTickArray
        );
    }
    for account in &remaining_accounts[bitmap_range.clone()] {
        require_eq!(
            account.data_len(),
            TickArrayBitmapExtension::LEN,
            ErrorCode::InvalidTickArrayBitmapExtensionAccount
        );
    }
    let tick_accounts = &remaining_accounts[tick_range.start..bitmap_range.end];
    let input_hook_accounts = &remaining_accounts[input_range];
    let output_hook_accounts = &remaining_accounts[output_range];

    let amount_result = exact_internal_with_hook_accounts(
        ctx.accounts,
        &tick_accounts,
        &input_hook_accounts,
        &output_hook_accounts,
        amount,
        sqrt_price_limit_x64,
        is_base_input,
    )?;
    if is_base_input {
        require_gte!(
            amount_result,
            other_amount_threshold,
            ErrorCode::TooLittleOutputReceived
        );
    } else {
        require_gte!(
            other_amount_threshold,
            amount_result,
            ErrorCode::TooMuchInputPaid
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::swap_v3_account_ranges;

    #[test]
    fn swap_v3_ranges_keep_tick_and_transfer_sections_distinct() {
        let (ticks, bitmap, input, output) = swap_v3_account_ranges(10, 4, 1, 3, 2).unwrap();
        assert_eq!(ticks, 0..4);
        assert_eq!(bitmap, 4..5);
        assert_eq!(input, 5..8);
        assert_eq!(output, 8..10);
    }

    #[test]
    fn swap_v3_rejects_invalid_or_unframed_slices() {
        assert!(swap_v3_account_ranges(2, 0, 2, 0, 0).is_err());
        assert!(swap_v3_account_ranges(1, 0, 0, 1, 0).is_err());
        assert!(swap_v3_account_ranges(3, 0, 0, 2, 2).is_err());
    }
}
