//! The fair-launch template: the pool's hooked-token vault is the buy source, and the launch
//! window limits what a buyer can take.
//!
//! Over the flow: ordinary swaps pass; three kinds of buy are refused with the hook's own codes (too
//! large, too many in one slot, too high a priority fee); then, once the window has closed, the
//! same oversized buy succeeds.

use fair_launch_hook::{
    config::counter_address, error::FairLaunchError, instruction::initialize, rule::Params,
};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::SplTransferLeg;

use super::{
    point_mint_at_hook, Direction, FollowUp, HookContext, HookSetup, Refusal, RejectionPlan,
};

pub struct FairLaunchHook {
    pub program_id: Pubkey,
    /// Seconds from setup until the window closes (a live cluster really waits that long).
    pub window_seconds: i64,
    pub max_buy: u64,
    pub max_buys_per_slot: u32,
    pub max_priority_micro_lamports: u64,
}

impl FairLaunchHook {
    /// Limits that the standard flow swaps (10 tokens) pass and the refusal swaps break.
    pub fn new(program_id: Pubkey, window_seconds: i64) -> Self {
        Self {
            program_id,
            window_seconds,
            max_buy: 100,
            max_buys_per_slot: 2,
            max_priority_micro_lamports: 1_000,
        }
    }

    /// A buy over `max_buy` (the pool pays out about as many tokens as it takes in).
    fn oversized_buy(&self) -> u64 {
        self.max_buy * 5
    }
}

impl HookSetup for FairLaunchHook {
    fn name(&self) -> &'static str {
        "fair-launch (buy caps, slot budget, fee cap)"
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        let params = Params {
            window_start: ctx.now,
            window_end: ctx.now + self.window_seconds,
            max_buy: self.max_buy,
            // Well above the trader's starting balance, so only the other checks bind here.
            max_wallet: 1_000_000,
            max_buys_per_slot: self.max_buys_per_slot,
            max_priority_micro_lamports: self.max_priority_micro_lamports,
        };
        vec![
            point_mint_at_hook(&ctx.hooked_mint, &ctx.payer, &self.program_id),
            initialize(
                &self.program_id,
                &ctx.payer,
                &ctx.payer,
                &ctx.hooked_mint,
                &ctx.vaults[0],
                params,
            ),
        ]
    }

    /// Only buying (the hooked token out of the pool) is bound.
    fn refusals(&self) -> Vec<Refusal> {
        let refuse = |plan, code: FairLaunchError| Refusal {
            direction: Direction::HookedOut,
            plan,
            code: code.code(),
        };
        vec![
            refuse(
                RejectionPlan::OverAmount {
                    amount_in: self.oversized_buy(),
                },
                FairLaunchError::PerBuyCapExceeded,
            ),
            refuse(
                RejectionPlan::RepeatInOneTransaction {
                    times: self.max_buys_per_slot as usize + 1,
                },
                FairLaunchError::TooManyBuysInSlot,
            ),
            refuse(
                RejectionPlan::HighPriorityFee {
                    amount_in: 10,
                    micro_lamports: self.max_priority_micro_lamports * 5,
                },
                FairLaunchError::PriorityFeeTooHigh,
            ),
        ]
    }

    /// The slot counter is a writable extra on every transfer of the mint, but only buys write it,
    /// so the flow must not expect it to change on every swap: name it as allowed, not as state.
    fn allowed_writable(&self, ctx: &HookContext, _leg: &SplTransferLeg) -> Vec<Pubkey> {
        vec![counter_address(&ctx.hooked_mint, &self.program_id).0]
    }

    fn follow_up(&self, _ctx: &HookContext) -> Vec<FollowUp> {
        vec![
            FollowUp::AdvanceTime(self.window_seconds as u64),
            FollowUp::Swap {
                label: "an oversized buy succeeds once the window has closed".into(),
                direction: Direction::HookedOut,
                amount_in: self.oversized_buy(),
            },
        ]
    }
}
