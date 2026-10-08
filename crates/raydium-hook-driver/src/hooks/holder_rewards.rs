//! The holder-rewards template: the quote token is the reward, the trader's hooked-token account is
//! the registered holder, and the pool's vault never earns.
//!
//! This hook never refuses a transfer. Over the flow: ordinary swaps settle the registered holder;
//! then the stream is funded, time passes, and the holder claims, and the flow checks that the
//! reward actually arrived.
//!
//! In its one-time mode ([`HolderRewardsHook::one_time`], a parent/child spin-off) the allocation can
//! be funded once: the flow also tries to fund it a second time and requires the hook's own refusal.

use holder_rewards_hook::{
    error::HolderRewardsError,
    instruction::{claim, fund, initialize, initialize_one_time, register},
    state::{global_address, record_address},
};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use spl_token_2022::instruction::{set_authority, AuthorityType};
use transfer_hook_sdk::SplTransferLeg;

use super::{point_mint_at_hook, FollowUp, HookContext, HookSetup, Refusal};

pub struct HolderRewardsHook {
    pub program_id: Pubkey,
    /// Reward tokens (of the quote mint) to stream to holders.
    pub reward_amount: u64,
    /// Seconds the reward is streamed over (a live cluster really waits that long).
    pub duration_seconds: u32,
    /// The allocation can be funded once (a spin-off) instead of topped up.
    pub one_time: bool,
}

impl HolderRewardsHook {
    /// An ongoing programme: the stream can be topped up.
    pub fn new(program_id: Pubkey, duration_seconds: u32) -> Self {
        Self {
            program_id,
            reward_amount: 4_000,
            duration_seconds,
            one_time: false,
        }
    }

    /// A one-time allocation (a spin-off): it can be funded once.
    pub fn one_time(program_id: Pubkey, duration_seconds: u32) -> Self {
        Self {
            one_time: true,
            ..Self::new(program_id, duration_seconds)
        }
    }
}

impl HookSetup for HolderRewardsHook {
    fn name(&self) -> &'static str {
        if self.one_time {
            "holder-rewards (one-time child allocation)"
        } else {
            "holder-rewards (balance x time quote rewards)"
        }
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        let setup = if self.one_time {
            initialize_one_time
        } else {
            initialize
        };
        vec![
            point_mint_at_hook(&ctx.hooked_mint, &ctx.payer, &self.program_id),
            // The rule assumes a fixed supply: minting is not a transfer, so a hook never sees it.
            set_authority(
                &spl_token_2022::id(),
                &ctx.hooked_mint,
                None,
                AuthorityType::MintTokens,
                &ctx.payer,
                &[],
            )
            .expect("revoke the mint authority"),
            setup(
                &self.program_id,
                &ctx.payer,
                &ctx.payer,
                &ctx.hooked_mint,
                &ctx.vaults[0],
                &ctx.quote_mint,
                &spl_token_2022::id(),
            ),
            register(
                &self.program_id,
                &ctx.payer,
                &ctx.hooked_mint,
                &ctx.trader_accounts[0],
            ),
        ]
    }

    /// Rewards never refuse a transfer.
    fn refusals(&self) -> Vec<Refusal> {
        Vec::new()
    }

    /// Every transfer carries three writable extras: the global and the two accounts' records.
    fn allowed_writable(&self, ctx: &HookContext, leg: &SplTransferLeg) -> Vec<Pubkey> {
        vec![
            global_address(&ctx.hooked_mint, &self.program_id).0,
            record_address(&leg.source, &self.program_id).0,
            record_address(&leg.destination, &self.program_id).0,
        ]
    }

    /// A UI fixture leaves the stream funded, so a holder who registers has something to claim.
    fn fixture_steps(&self, ctx: &HookContext) -> Vec<(String, Vec<Instruction>)> {
        let [_, quote_account] = ctx.trader_accounts;
        vec![
            (
                "mint the reward tokens".to_string(),
                vec![crate::token::mint_to_instruction(
                    &ctx.quote_mint,
                    &quote_account,
                    &ctx.payer,
                    self.reward_amount,
                )],
            ),
            (
                "fund the reward stream with quote tokens".to_string(),
                vec![fund(
                    &self.program_id,
                    &ctx.payer,
                    &quote_account,
                    &ctx.hooked_mint,
                    &ctx.quote_mint,
                    &spl_token_2022::id(),
                    self.reward_amount,
                    self.duration_seconds,
                )],
            ),
        ]
    }

    fn follow_up(&self, ctx: &HookContext) -> Vec<FollowUp> {
        let [hooked_account, quote_account] = ctx.trader_accounts;
        let mut steps = vec![
            FollowUp::Remember {
                name: "quote before funding".into(),
                account: quote_account,
            },
            FollowUp::Send {
                label: "fund the reward stream with quote tokens".into(),
                instructions: vec![fund(
                    &self.program_id,
                    &ctx.payer,
                    &quote_account,
                    &ctx.hooked_mint,
                    &ctx.quote_mint,
                    &spl_token_2022::id(),
                    self.reward_amount,
                    self.duration_seconds,
                )],
                signers: Vec::new(),
            },
            FollowUp::AdvanceTime(self.duration_seconds as u64),
            FollowUp::Send {
                label: "the registered holder claims its quote rewards".into(),
                instructions: vec![claim(
                    &self.program_id,
                    &ctx.payer,
                    &ctx.hooked_mint,
                    &hooked_account,
                    &quote_account,
                    &ctx.quote_mint,
                    &spl_token_2022::id(),
                )],
                signers: Vec::new(),
            },
            // The holder is the only registered account (the pool never earns), so it takes the
            // whole stream, less dust: the stream pays `floor(amount / duration)` per second, so
            // `amount % duration` is never paid out, plus a few units of index rounding.
            FollowUp::ExpectChange {
                label: "the quote rewards arrived (net of what was funded)".into(),
                account: quote_account,
                since: "quote before funding".into(),
                min: -5 - (self.reward_amount % u64::from(self.duration_seconds.max(1))) as i64,
                max: 0,
            },
        ];
        if self.one_time {
            // Right after the funding step (`Remember`, then the `Send` that funds), try to fund
            // again. One token less than the first funding: the program refuses any second funding
            // before it looks at the amount, and an identical transaction under the same blockhash
            // would be dropped as a duplicate before the hook ever ran.
            steps.insert(
                2,
                FollowUp::SendExpectFailure {
                    label: "a second funding of the one-time allocation is refused".into(),
                    instructions: vec![fund(
                        &self.program_id,
                        &ctx.payer,
                        &quote_account,
                        &ctx.hooked_mint,
                        &ctx.quote_mint,
                        &spl_token_2022::id(),
                        self.reward_amount - 1,
                        self.duration_seconds,
                    )],
                    signers: Vec::new(),
                    code: HolderRewardsError::AlreadyFunded.code(),
                },
            );
        }
        steps
    }
}
