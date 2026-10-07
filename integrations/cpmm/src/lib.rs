//! MODEL ONLY: not a Raydium CPI.
//!
//! Plans the two transfer legs of a CPMM `swap_base_input` and delegates the
//! framing to the SDK's authentic framers. This crate never builds or sends a
//! Raydium instruction by itself. Live entrypoints are `swap_base_input_v2`
//! (hooked) and the unchanged V1; deposits and withdrawals are rejected by the
//! program for hooked mints, so they have no plan here.

#![forbid(unsafe_code)]

use std::future::Future;

use transfer_hook_sdk::solana_program::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{
    frame_cpmm_or_passthrough, resolve_leg, CpmmSwapAccounts, FetchError, FrameError, FramedSwap,
    LegError, LegHook, LegRole, ResolveOptions, SplAccount, SplTransferLeg,
};

/// The two resolved transfer legs of a CPMM base-input swap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpmmHookPlan {
    pub input: LegHook,
    pub output: LegHook,
}

/// The input (trader to input vault) and output (output vault to trader)
/// transfers implied by the fixed accounts of a CPMM swap.
pub fn cpmm_swap_legs(
    accounts: &CpmmSwapAccounts,
    amount_in: u64,
    expected_amount_out: u64,
) -> (SplTransferLeg, SplTransferLeg) {
    (
        SplTransferLeg {
            source: accounts.input_token_account,
            mint: accounts.input_token_mint,
            destination: accounts.input_vault,
            authority: accounts.payer,
            amount: amount_in,
        },
        SplTransferLeg {
            source: accounts.output_vault,
            mint: accounts.output_token_mint,
            destination: accounts.output_token_account,
            authority: accounts.authority,
            amount: expected_amount_out,
        },
    )
}

/// Resolve both legs, each with its own options (a launch policy pins the hook
/// of its own mint only, not of the counterpart token). If either leg fails
/// nothing is returned and the error names the leg.
pub async fn plan_cpmm_swap_base_input<F, Fut, E>(
    input: SplTransferLeg,
    output: SplTransferLeg,
    input_options: &ResolveOptions,
    output_options: &ResolveOptions,
    fetch: F,
) -> Result<CpmmHookPlan, LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let input = resolve_leg(LegRole::Input, input, input_options, &fetch).await?;
    let output = resolve_leg(LegRole::Output, output, output_options, &fetch).await?;
    Ok(CpmmHookPlan { input, output })
}

impl CpmmHookPlan {
    pub fn hook_account_count(&self) -> usize {
        self.input.account_count() + self.output.account_count()
    }

    /// Frame a V1 `swap_base_input` into `swap_base_input_v2` if either leg is
    /// hooked; a swap with no hook stays the byte-identical V1 (`Ok(None)`).
    pub fn frame(&self, instruction: &mut Instruction) -> Result<Option<FramedSwap>, FrameError> {
        frame_cpmm_or_passthrough(instruction, &self.input, &self.output)
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
    use transfer_hook_sdk::spl_tlv_account_resolution::account::ExtraAccountMeta;
    use transfer_hook_sdk::{
        build_cpmm_swap_base_input_v1,
        testing::{block_on, MemoryChain},
        CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
    };

    fn accounts() -> CpmmSwapAccounts {
        CpmmSwapAccounts {
            payer: Pubkey::new_unique(),
            authority: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            pool_state: Pubkey::new_unique(),
            input_token_account: Pubkey::new_unique(),
            output_token_account: Pubkey::new_unique(),
            input_vault: Pubkey::new_unique(),
            output_vault: Pubkey::new_unique(),
            input_token_program: transfer_hook_sdk::spl_token_2022::id(),
            output_token_program: transfer_hook_sdk::spl_token_2022::id(),
            input_token_mint: Pubkey::new_unique(),
            output_token_mint: Pubkey::new_unique(),
            observation_state: Pubkey::new_unique(),
        }
    }

    fn extra(key: &Pubkey) -> ExtraAccountMeta {
        ExtraAccountMeta::new_with_pubkey(key, false, false).unwrap()
    }

    #[test]
    fn plans_two_independent_legs_and_frames_the_swap() {
        let accounts = accounts();
        let shared = Pubkey::new_unique();
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(
            accounts.input_token_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&shared)],
        );
        chain.add_hooked_mint(
            accounts.output_token_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&shared)],
        );
        let (input, output) = cpmm_swap_legs(&accounts, 100, 90);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();

        // Duplicate accounts across legs are kept per leg, never merged.
        assert_eq!(plan.hook_account_count(), 6);
        assert_ne!(
            plan.input.slice().unwrap().hook_program(),
            plan.output.slice().unwrap().hook_program()
        );

        let mut instruction =
            build_cpmm_swap_base_input_v1(Pubkey::new_unique(), &accounts, 100, 1);
        let framed = plan.frame(&mut instruction).unwrap().unwrap();
        assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR);
        assert_eq!(
            (framed.input_hook_accounts, framed.output_hook_accounts),
            (3, 3)
        );
        assert_eq!(instruction.accounts.len(), 13 + 6);
        block_on(plan.verify_unchanged(chain.fetcher())).unwrap();
    }

    #[test]
    fn unhooked_swap_is_left_as_v1() {
        let accounts = accounts();
        let mut chain = MemoryChain::new();
        chain.add_classic_mint(accounts.input_token_mint);
        chain.add_unhooked_token_2022_mint(accounts.output_token_mint);
        let (input, output) = cpmm_swap_legs(&accounts, 100, 90);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        let mut instruction =
            build_cpmm_swap_base_input_v1(Pubkey::new_unique(), &accounts, 100, 1);
        let before = instruction.clone();
        assert_eq!(plan.frame(&mut instruction), Ok(None));
        assert_eq!(instruction, before);
        assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR);
        assert_eq!(plan.hook_account_count(), 0);
    }

    #[test]
    fn output_leg_failure_is_attributed_and_returns_no_plan() {
        let accounts = accounts();
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(accounts.input_token_mint, Pubkey::new_unique(), None, &[]);
        // The output mint is missing from the chain.
        let (input, output) = cpmm_swap_legs(&accounts, 100, 90);
        let error = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap_err();
        assert_eq!(error.leg, LegRole::Output);
        assert_eq!(error.mint, accounts.output_token_mint);
    }
}
