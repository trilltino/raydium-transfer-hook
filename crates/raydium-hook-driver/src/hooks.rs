//! Hook setup providers. A Transfer Hook defines how it executes, not how it is initialised, so
//! setup is delegated to a provider. The flows and the Raydium builders only see this trait; a
//! new hook adds a provider and nothing else changes.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use spl_token_2022::extension::transfer_hook::instruction as transfer_hook_instruction;

/// How a flow can make a hooked swap fail, deterministically, on any cluster.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectionPlan {
    /// A swap whose input amount is above the hook's per-transfer limit.
    OverAmount { amount_in: u64 },
    /// `times` swaps inside one transaction (more than the hook allows per slot per mint).
    RepeatInOneTransaction { times: usize },
}

pub trait HookSetup {
    fn name(&self) -> &'static str;
    fn program_id(&self) -> Pubkey;
    /// Run by the mint's TransferHook authority: point the mint at the hook, then initialise the
    /// hook's per-mint state.
    fn enable_instructions(
        &self,
        mint: &Pubkey,
        authority: &Pubkey,
        payer: &Pubkey,
    ) -> Vec<Instruction>;
    /// The custom error code the hook returns when it refuses a transfer.
    fn rejection_code(&self) -> u32;
    fn rejection_plan(&self) -> RejectionPlan;
    /// An account the hook writes on every allowed transfer, if it keeps state.
    fn state_account(&self, mint: &Pubkey) -> Option<Pubkey>;
}

fn point_mint_at_hook(mint: &Pubkey, authority: &Pubkey, program: &Pubkey) -> Instruction {
    transfer_hook_instruction::update(&spl_token_2022::id(), mint, authority, &[], Some(*program))
        .expect("TransferHook update")
}

/// The repository's reference hook: a per-transfer maximum amount.
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

    fn enable_instructions(
        &self,
        mint: &Pubkey,
        authority: &Pubkey,
        payer: &Pubkey,
    ) -> Vec<Instruction> {
        vec![
            point_mint_at_hook(mint, authority, &self.program_id),
            reference_hook_onchain::initialize_hook_instruction(
                self.program_id,
                *mint,
                *authority,
                *payer,
                &reference_hook_onchain::InitializeHookArgs::max_transfer(
                    reference_hook_onchain::AuthorityMode::ExtensionAuthority,
                    self.max_transfer,
                    Pubkey::default(),
                ),
            ),
        ]
    }

    fn rejection_code(&self) -> u32 {
        reference_hook_onchain::HookError::TransferExceedsLimit.code()
    }

    fn rejection_plan(&self) -> RejectionPlan {
        RejectionPlan::OverAmount {
            amount_in: self.max_transfer.saturating_add(100),
        }
    }

    fn state_account(&self, _mint: &Pubkey) -> Option<Pubkey> {
        None
    }
}

/// The unrelated third-party-style hook: at most `max_per_slot` transfers per slot, counted in a
/// writable stats account (so it mutates state and needs two extra accounts).
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

    fn enable_instructions(
        &self,
        mint: &Pubkey,
        authority: &Pubkey,
        payer: &Pubkey,
    ) -> Vec<Instruction> {
        vec![
            point_mint_at_hook(mint, authority, &self.program_id),
            arbitrary_test_hook::init_instruction(
                self.program_id,
                *payer,
                *mint,
                *authority,
                self.max_per_slot,
            ),
        ]
    }

    fn rejection_code(&self) -> u32 {
        arbitrary_test_hook::ArbError::SlotLimitExceeded.code()
    }

    fn rejection_plan(&self) -> RejectionPlan {
        RejectionPlan::RepeatInOneTransaction {
            times: self.max_per_slot as usize + 1,
        }
    }

    fn state_account(&self, mint: &Pubkey) -> Option<Pubkey> {
        Some(arbitrary_test_hook::stats_address(mint, &self.program_id).0)
    }
}
