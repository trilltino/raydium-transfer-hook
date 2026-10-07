//! The repository's reference hook: a per-transfer maximum amount.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use super::{point_mint_at_hook, Direction, HookContext, HookSetup, Refusal, RejectionPlan};

pub struct ReferenceHook {
    pub program_id: Pubkey,
    pub max_transfer: u64,
}

impl HookSetup for ReferenceHook {
    fn name(&self) -> &'static str {
        "reference-hook (max transfer)"
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        vec![
            point_mint_at_hook(&ctx.hooked_mint, &ctx.payer, &self.program_id),
            reference_hook_onchain::initialize_hook_instruction(
                self.program_id,
                ctx.hooked_mint,
                ctx.payer,
                ctx.payer,
                &reference_hook_onchain::InitializeHookArgs::max_transfer(
                    reference_hook_onchain::AuthorityMode::ExtensionAuthority,
                    self.max_transfer,
                    Pubkey::default(),
                ),
            ),
        ]
    }

    fn refusals(&self) -> Vec<Refusal> {
        let code = reference_hook_onchain::HookError::TransferExceedsLimit.code();
        let plan = RejectionPlan::OverAmount {
            amount_in: self.max_transfer.saturating_add(100),
        };
        vec![
            Refusal {
                direction: Direction::HookedIn,
                plan,
                code,
            },
            Refusal {
                direction: Direction::HookedOut,
                plan,
                code,
            },
        ]
    }
}
