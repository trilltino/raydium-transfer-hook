//! The parent/spin-off template: the same balance-time accounting as loyalty-rewards, with the
//! child allocation (the quote token, here) funded exactly once.
//!
//! Over the flow: ordinary swaps settle the registered parent holder; the allocation is funded; a
//! second funding is refused with the hook's own code; time passes; the holder claims and the
//! flow checks the child tokens arrived.

use parent_spin_off_hook::{error::SpinOffError, instruction::fund};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::SplTransferLeg;

use super::{FollowUp, HookContext, HookSetup, LoyaltyRewardsHook, Refusal};

pub struct ParentSpinOffHook {
    /// The shared accounting setup, pointed at the spin-off program.
    inner: LoyaltyRewardsHook,
}

impl ParentSpinOffHook {
    pub fn new(program_id: Pubkey, duration_seconds: u32) -> Self {
        Self {
            inner: LoyaltyRewardsHook::new(program_id, duration_seconds),
        }
    }
}

impl HookSetup for ParentSpinOffHook {
    fn name(&self) -> &'static str {
        "parent-spin-off (one-time child allocation)"
    }

    fn program_id(&self) -> Pubkey {
        self.inner.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        self.inner.enable_instructions(ctx)
    }

    fn refusals(&self) -> Vec<Refusal> {
        self.inner.refusals()
    }

    fn allowed_writable(&self, ctx: &HookContext, leg: &SplTransferLeg) -> Vec<Pubkey> {
        self.inner.allowed_writable(ctx, leg)
    }

    fn follow_up(&self, ctx: &HookContext) -> Vec<FollowUp> {
        let mut steps = self.inner.follow_up(ctx);
        // Right after the funding step (`Remember`, then the `Send` that funds), try to fund again.
        let [_, quote_account] = ctx.trader_accounts;
        steps.insert(
            2,
            FollowUp::SendExpectFailure {
                label: "a second funding of the spin-off is refused".into(),
                instructions: vec![fund(
                    &self.inner.program_id,
                    &ctx.payer,
                    &quote_account,
                    &ctx.hooked_mint,
                    &ctx.quote_mint,
                    &spl_token_2022::id(),
                    self.inner.reward_amount,
                    self.inner.duration_seconds,
                )],
                signers: Vec::new(),
                code: SpinOffError::AlreadyFunded.code(),
            },
        );
        steps
    }
}
