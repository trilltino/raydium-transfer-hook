//! Resolving a swap's two legs and framing the instruction.

use std::future::Future;

use transfer_hook_sdk::solana_program::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{
    frame_cpmm_or_passthrough, resolve_leg, FetchError, FrameError, FramedSwap, LegError, LegHook,
    LegRole, ResolveOptions, SplAccount, SplTransferLeg,
};

/// The two resolved transfer legs of a CPMM base-input swap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpmmHookPlan {
    pub input: LegHook,
    pub output: LegHook,
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
