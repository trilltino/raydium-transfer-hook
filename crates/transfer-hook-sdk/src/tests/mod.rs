//! Unit tests. Every resolution test uses real Token-2022 mint bytes and real
//! `spl-tlv-account-resolution` validation lists, never a fake PDA.

mod frame_tests;
mod golden_tests;
mod resolve_tests;

use solana_program::pubkey::Pubkey;
use spl_tlv_account_resolution::account::ExtraAccountMeta;

use crate::{
    abi::{ClmmSwapAccounts, CpmmSwapAccounts},
    resolve::SplTransferLeg,
    testing::MemoryChain,
};

/// One hooked mint on an in-memory chain with a configurable list.
pub(crate) struct Setup {
    pub chain: MemoryChain,
    pub mint: Pubkey,
    pub hook: Pubkey,
    pub source: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
}

pub(crate) fn setup(extras: &[ExtraAccountMeta]) -> Setup {
    let mint = Pubkey::new_unique();
    let hook = Pubkey::new_unique();
    let mut chain = MemoryChain::new();
    chain.add_hooked_mint(mint, hook, Some(Pubkey::new_unique()), extras);
    Setup {
        chain,
        mint,
        hook,
        source: Pubkey::new_unique(),
        destination: Pubkey::new_unique(),
        authority: Pubkey::new_unique(),
    }
}

impl Setup {
    pub fn leg(&self, amount: u64) -> SplTransferLeg {
        SplTransferLeg {
            source: self.source,
            mint: self.mint,
            destination: self.destination,
            authority: self.authority,
            amount,
        }
    }
}

pub(crate) fn cpmm_accounts() -> CpmmSwapAccounts {
    CpmmSwapAccounts {
        payer: Pubkey::new_unique(),
        authority: Pubkey::new_unique(),
        amm_config: Pubkey::new_unique(),
        pool_state: Pubkey::new_unique(),
        input_token_account: Pubkey::new_unique(),
        output_token_account: Pubkey::new_unique(),
        input_vault: Pubkey::new_unique(),
        output_vault: Pubkey::new_unique(),
        input_token_program: spl_token_2022::id(),
        output_token_program: spl_token_2022::id(),
        input_token_mint: Pubkey::new_unique(),
        output_token_mint: Pubkey::new_unique(),
        observation_state: Pubkey::new_unique(),
    }
}

pub(crate) fn clmm_accounts() -> ClmmSwapAccounts {
    ClmmSwapAccounts {
        payer: Pubkey::new_unique(),
        amm_config: Pubkey::new_unique(),
        pool_state: Pubkey::new_unique(),
        input_token_account: Pubkey::new_unique(),
        output_token_account: Pubkey::new_unique(),
        input_vault: Pubkey::new_unique(),
        output_vault: Pubkey::new_unique(),
        observation_state: Pubkey::new_unique(),
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        memo_program: Pubkey::new_unique(),
        input_vault_mint: Pubkey::new_unique(),
        output_vault_mint: Pubkey::new_unique(),
    }
}
