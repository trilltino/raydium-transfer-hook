//! The repository's reference hook, which is the starter template with its default rule: a
//! per-transfer maximum amount.

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
            transfer_hook_starter::initialize_hook_instruction(
                self.program_id,
                ctx.hooked_mint,
                ctx.payer,
                ctx.payer,
                &transfer_hook_starter::InitializeHookArgs::max_transfer(
                    transfer_hook_starter::AuthorityMode::ExtensionAuthority,
                    self.max_transfer,
                    Pubkey::default(),
                ),
            ),
        ]
    }

    fn refusals(&self) -> Vec<Refusal> {
        let code = transfer_hook_starter::HookError::TransferExceedsLimit.code();
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
