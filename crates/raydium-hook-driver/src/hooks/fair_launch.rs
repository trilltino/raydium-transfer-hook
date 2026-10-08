//! The fair-launch template: the pool's hooked-token vault is the buy source, and the launch
//! window limits what a buyer can take.
//!
//! Over the flow: ordinary swaps pass; each enabled limit is broken by a buy that is refused with
//! the hook's own code; then, once the window has closed, the same oversized buy succeeds.
//!
//! The same program also runs as an anti-bundle budget ([`FairLaunchHook::per_slot_only`]): only the
//! per-slot budget is on, and the rule never ends, so a bundle of too many buys in one slot is
//! refused as a whole.

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
    /// Seconds from setup until the window closes (a live cluster really waits that long). Unused
    /// when `no_end` is set.
    pub window_seconds: i64,
    /// `0` switches the per-buy cap off.
    pub max_buy: u64,
    /// `0` switches the per-account cap off.
    pub max_wallet: u64,
    /// `0` switches the per-slot budget off.
    pub max_buys_per_slot: u32,
    /// `0` switches the priority-fee check off.
    pub max_priority_micro_lamports: u64,
    /// The rule never ends, instead of closing after `window_seconds`.
    pub no_end: bool,
}

impl FairLaunchHook {
    /// Limits that the standard flow swaps (10 tokens) pass and the refusal swaps break.
    pub fn new(program_id: Pubkey, window_seconds: i64) -> Self {
        Self {
            program_id,
            window_seconds,
            max_buy: 100,
            // Well above the trader's starting balance, so only the other checks bind here.
            max_wallet: 1_000_000,
            max_buys_per_slot: 2,
            max_priority_micro_lamports: 1_000,
            no_end: false,
        }
    }

    /// The anti-bundle setting: only a per-slot budget of two buys, for the whole of time.
    pub fn per_slot_only(program_id: Pubkey) -> Self {
        Self {
            program_id,
            window_seconds: 0,
            max_buy: 0,
            max_wallet: 0,
            max_buys_per_slot: 2,
            max_priority_micro_lamports: 0,
            no_end: true,
        }
    }

    /// A buy over `max_buy` (the pool pays out about as many tokens as it takes in).
    fn oversized_buy(&self) -> u64 {
        self.max_buy * 5
    }
}

impl HookSetup for FairLaunchHook {
    fn name(&self) -> &'static str {
        if self.no_end {
            "fair-launch (per-slot budget only, the anti-bundle setting)"
        } else {
            "fair-launch (buy caps, slot budget, fee cap)"
        }
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        let (window_start, window_end) = if self.no_end {
            (0, i64::MAX)
        } else {
            (ctx.now, ctx.now + self.window_seconds)
        };
        let params = Params {
            window_start,
            window_end,
            max_buy: self.max_buy,
            max_wallet: self.max_wallet,
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
                &[ctx.vaults[0]],
                params,
            ),
        ]
    }

    /// Only buying (the hooked token out of the pool) is bound; one refusal per enabled limit.
    fn refusals(&self) -> Vec<Refusal> {
        let refuse = |plan, code: FairLaunchError| Refusal {
            direction: Direction::HookedOut,
            plan,
            code: code.code(),
        };
        let mut refusals = Vec::new();
        if self.max_buy > 0 {
            refusals.push(refuse(
                RejectionPlan::OverAmount {
                    amount_in: self.oversized_buy(),
                },
                FairLaunchError::PerBuyCapExceeded,
            ));
        }
        if self.max_buys_per_slot > 0 {
            refusals.push(refuse(
                RejectionPlan::RepeatInOneTransaction {
                    times: self.max_buys_per_slot as usize + 1,
                },
                FairLaunchError::TooManyBuysInSlot,
            ));
        }
        if self.max_priority_micro_lamports > 0 {
            refusals.push(refuse(
                RejectionPlan::HighPriorityFee {
                    amount_in: 10,
                    micro_lamports: self.max_priority_micro_lamports * 5,
                },
                FairLaunchError::PriorityFeeTooHigh,
            ));
        }
        refusals
    }

    /// The slot counter is a writable extra on every transfer of the mint, but only buys write it,
    /// so the flow must not expect it to change on every swap: name it as allowed, not as state.
    fn allowed_writable(&self, ctx: &HookContext, _leg: &SplTransferLeg) -> Vec<Pubkey> {
        vec![counter_address(&ctx.hooked_mint, &self.program_id).0]
    }

    fn follow_up(&self, _ctx: &HookContext) -> Vec<FollowUp> {
        // Once the window closes an oversized buy goes through; without a window or a size cap
        // there is nothing to show.
        if self.no_end || self.max_buy == 0 {
            return Vec::new();
        }
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
