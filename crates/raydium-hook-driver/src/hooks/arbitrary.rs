//! The unrelated third-party-style hook: at most `max_per_slot` transfers per slot, counted in a
//! writable stats account (so it mutates state and needs two extra accounts).

use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use super::{point_mint_at_hook, Direction, HookContext, HookSetup, Refusal, RejectionPlan};

pub struct ArbitraryHook {
    pub program_id: Pubkey,
    pub max_per_slot: u32,
}

impl HookSetup for ArbitraryHook {
    fn name(&self) -> &'static str {
        "arbitrary-test-hook (per-slot cap + counter)"
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        vec![
            point_mint_at_hook(&ctx.hooked_mint, &ctx.payer, &self.program_id),
            arbitrary_test_hook::init_instruction(
                self.program_id,
                ctx.payer,
                ctx.hooked_mint,
                ctx.payer,
                self.max_per_slot,
            ),
        ]
    }

    fn refusals(&self) -> Vec<Refusal> {
        let code = arbitrary_test_hook::ArbError::SlotLimitExceeded.code();
        let plan = RejectionPlan::RepeatInOneTransaction {
            times: self.max_per_slot as usize + 1,
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

    fn state_account(&self, mint: &Pubkey) -> Option<Pubkey> {
        Some(arbitrary_test_hook::stats_address(mint, &self.program_id).0)
    }
}
