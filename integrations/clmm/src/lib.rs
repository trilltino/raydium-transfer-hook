//! MODEL ONLY: not a Raydium CPI.
//!
//! Plans the two transfer legs of a CLMM swap and delegates framing to the
//! SDK's `frame_clmm_swap_v3`. The live hooked entrypoint is `swap_v3`; this
//! crate does not execute it, and no CLMM hooked swap has been run on a
//! runtime. Tick arrays and the bitmap extension stay outside the per-leg hook
//! slices and are only counted here.

#![forbid(unsafe_code)]

use std::future::Future;

use transfer_hook_sdk::solana_program::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{
    frame_clmm_or_passthrough, resolve_leg, ClmmSwapAccounts, FetchError, FrameError, FramedSwap,
    LegError, LegHook, LegRole, ResolveOptions, SplAccount, SplTransferLeg,
};

/// A CLMM `swap_v3` plan: the tick/bitmap prefix sizes plus the two resolved legs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClmmSwapV3Plan {
    /// Number of tick-array accounts in the instruction's remaining accounts.
    pub ticks: u16,
    /// Number of bitmap-extension accounts (zero or one).
    pub bitmaps: u16,
    pub input: LegHook,
    pub output: LegHook,
}

/// The input (trader to input vault) and output (output vault to trader)
/// transfers implied by the fixed accounts of a CLMM swap. The pool state
/// authorizes the output transfer.
pub fn clmm_swap_legs(
    accounts: &ClmmSwapAccounts,
    amount_in: u64,
    expected_amount_out: u64,
) -> (SplTransferLeg, SplTransferLeg) {
    (
        SplTransferLeg {
            source: accounts.input_token_account,
            mint: accounts.input_vault_mint,
            destination: accounts.input_vault,
            authority: accounts.payer,
            amount: amount_in,
        },
        SplTransferLeg {
            source: accounts.output_vault,
            mint: accounts.output_vault_mint,
            destination: accounts.output_token_account,
            authority: accounts.pool_state,
            amount: expected_amount_out,
        },
    )
}

/// Resolve both legs, each with its own options; a failure names the failing leg
/// and nothing is returned.
pub async fn plan_clmm_swap_v3<F, Fut, E>(
    ticks: u16,
    bitmaps: u16,
    input: SplTransferLeg,
    output: SplTransferLeg,
    input_options: &ResolveOptions,
    output_options: &ResolveOptions,
    fetch: F,
) -> Result<ClmmSwapV3Plan, LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let input = resolve_leg(LegRole::Input, input, input_options, &fetch).await?;
    let output = resolve_leg(LegRole::Output, output, output_options, &fetch).await?;
    Ok(ClmmSwapV3Plan {
        ticks,
        bitmaps,
        input,
        output,
    })
}

impl ClmmSwapV3Plan {
    pub fn hook_account_count(&self) -> usize {
        self.input.account_count() + self.output.account_count()
    }

    /// Frame a `swap_v2` into `swap_v3` if either leg is hooked; an unhooked
    /// swap stays the byte-identical `swap_v2` (`Ok(None)`).
    pub fn frame(&self, instruction: &mut Instruction) -> Result<Option<FramedSwap>, FrameError> {
        frame_clmm_or_passthrough(
            instruction,
            self.ticks,
            self.bitmaps,
            &self.input,
            &self.output,
        )
    }

    /// Re-check both hooks immediately before signing.
    pub async fn verify_unchanged<F, Fut, E>(&self, fetch: F) -> Result<(), LegError>
    where
        F: Fn(Pubkey) -> Fut,
        Fut: Future<Output = Result<Option<SplAccount>, E>>,
        E: Into<FetchError>,
    {
        transfer_hook_sdk::verify_legs_unchanged(&[&self.input, &self.output], fetch).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
