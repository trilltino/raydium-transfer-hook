//! The anti-bundle template: the pool's vault of the hooked token is the one recognised venue, and
//! each slot gets a small budget of buys from it.
//!
//! Over the flow: ordinary swaps pass; a transaction with one swap more than the budget allows,
//! all in the same slot, is refused with the hook's own code and rolls back as a whole.

use anti_bundle_hook::{
    error::AntiBundleError, instruction::initialize, rule::Params, state::counter_address,
};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::SplTransferLeg;

use super::{point_mint_at_hook, Direction, HookContext, HookSetup, Refusal, RejectionPlan};

pub struct AntiBundleHook {
    pub program_id: Pubkey,
    pub max_buys_per_slot: u16,
}

impl AntiBundleHook {
    /// A budget of two buys per slot that never expires.
    pub fn new(program_id: Pubkey) -> Self {
        Self {
            program_id,
            max_buys_per_slot: 2,
        }
    }
}

impl HookSetup for AntiBundleHook {
    fn name(&self) -> &'static str {
        "anti-bundle (per-slot buy budget)"
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        vec![
            point_mint_at_hook(&ctx.hooked_mint, &ctx.payer, &self.program_id),
            initialize(
                &self.program_id,
                &ctx.payer,
                &ctx.payer,
                &ctx.hooked_mint,
                &[ctx.vaults[0]],
                Params {
                    active_until: 0,
                    max_buys_per_slot: self.max_buys_per_slot,
                },
            ),
        ]
    }

    /// Only buying (the hooked token out of the pool) is counted.
    fn refusals(&self) -> Vec<Refusal> {
        vec![Refusal {
            direction: Direction::HookedOut,
            plan: RejectionPlan::RepeatInOneTransaction {
                times: self.max_buys_per_slot as usize + 1,
            },
            code: AntiBundleError::TooManyBuysInSlot.code(),
        }]
    }

    /// The slot counter is a writable extra on every transfer of the mint, but only buys write it,
    /// so it is named as allowed rather than as state the flow expects to change on each swap.
    fn allowed_writable(&self, ctx: &HookContext, _leg: &SplTransferLeg) -> Vec<Pubkey> {
        vec![counter_address(&ctx.hooked_mint, &self.program_id).0]
    }
}
