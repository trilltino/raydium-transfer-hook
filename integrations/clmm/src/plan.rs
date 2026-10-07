//! Resolving a swap's two legs and framing the instruction.

use std::future::Future;

use transfer_hook_sdk::solana_program::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{
    frame_clmm_or_passthrough, resolve_leg, FetchError, FrameError, FramedSwap, LegError, LegHook,
    LegRole, ResolveOptions, SplAccount, SplTransferLeg,
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
