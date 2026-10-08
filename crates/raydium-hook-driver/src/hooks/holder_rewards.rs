//! The loyalty-rewards template: the quote token is the reward, the trader's hooked-token account
//! is the registered holder, and the pool's vault never earns.
//!
//! This hook never refuses a transfer. Over the flow: ordinary swaps settle the registered
//! holder; then the stream is funded, time passes, and the holder claims, and the flow checks
//! that the reward actually arrived.

use loyalty_rewards_hook::{
    instruction::{claim, fund, initialize, register},
    state::{global_address, record_address},
};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use spl_token_2022::instruction::{set_authority, AuthorityType};
use transfer_hook_sdk::SplTransferLeg;

use super::{point_mint_at_hook, FollowUp, HookContext, HookSetup, Refusal};

pub struct LoyaltyRewardsHook {
    pub program_id: Pubkey,
    /// Reward tokens (of the quote mint) to stream to holders.
    pub reward_amount: u64,
    /// Seconds the reward is streamed over (a live cluster really waits that long).
    pub duration_seconds: u32,
}

impl LoyaltyRewardsHook {
    pub fn new(program_id: Pubkey, duration_seconds: u32) -> Self {
        Self {
            program_id,
            reward_amount: 4_000,
            duration_seconds,
        }
    }
}

impl HookSetup for LoyaltyRewardsHook {
    fn name(&self) -> &'static str {
        "loyalty-rewards (balance x time quote rewards)"
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
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
            initialize(
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

    fn follow_up(&self, ctx: &HookContext) -> Vec<FollowUp> {
        let [hooked_account, quote_account] = ctx.trader_accounts;
        vec![
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
        ]
    }
}
