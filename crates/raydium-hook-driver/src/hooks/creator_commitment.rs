//! The creator-commitment template: the trader's hooked-token account plays the creator, with most
//! of its balance locked on a short cliff-and-linear schedule.
//!
//! Over the flow: the standard swaps (small, above the floor) pass; a sale that would breach the
//! floor is refused with the hook's own code; then, once the schedule has run out, the same sale
//! succeeds.

use creator_commitment_hook::{error::CommitmentError, instruction::initialize, rule::Schedule};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use super::{
    point_mint_at_hook, Direction, FollowUp, HookContext, HookSetup, Refusal, RejectionPlan,
};

pub struct CreatorCommitmentHook {
    pub program_id: Pubkey,
    /// Tokens of the creator account locked at the start.
    pub locked_total: u64,
    /// Seconds from setup until the cliff.
    pub cliff_seconds: i64,
    /// Seconds from setup until everything is unlocked.
    pub vest_seconds: i64,
}

impl CreatorCommitmentHook {
    /// Locks 9,000 of the trader's 10,000 tokens; vests over `vest_seconds` (a live cluster really
    /// waits that long, so keep it short there).
    pub fn new(program_id: Pubkey, vest_seconds: i64) -> Self {
        Self {
            program_id,
            locked_total: 9_000,
            cliff_seconds: vest_seconds / 3,
            vest_seconds,
        }
    }

    /// The sale the floor must refuse (and, after the end, allow): nearly the whole balance.
    const BREACHING_SALE: u64 = 9_900;
}

impl HookSetup for CreatorCommitmentHook {
    fn name(&self) -> &'static str {
        "creator-commitment (vesting floor)"
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        let schedule = Schedule {
            locked_total: self.locked_total,
            start: ctx.now,
            cliff: ctx.now + self.cliff_seconds,
            end: ctx.now + self.vest_seconds,
        };
        vec![
            point_mint_at_hook(&ctx.hooked_mint, &ctx.payer, &self.program_id),
            initialize(
                &self.program_id,
                &ctx.payer,
                &ctx.payer,
                &ctx.hooked_mint,
                &ctx.trader_accounts[0],
                schedule,
            ),
        ]
    }

    /// Only a sale out of the creator account is bound; buying is never refused.
    fn refusals(&self) -> Vec<Refusal> {
        vec![Refusal {
            direction: Direction::HookedIn,
            plan: RejectionPlan::OverAmount {
                amount_in: Self::BREACHING_SALE,
            },
            code: CommitmentError::VestingFloorBreached.code(),
        }]
    }

    fn follow_up(&self, _ctx: &HookContext) -> Vec<FollowUp> {
        vec![
            FollowUp::AdvanceTime(self.vest_seconds as u64),
            FollowUp::Swap {
                label: "creator sells past the old floor once the schedule has ended".into(),
                direction: Direction::HookedIn,
                amount_in: Self::BREACHING_SALE,
            },
        ]
    }
}
