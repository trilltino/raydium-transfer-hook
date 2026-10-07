//! Raydium CPMM (CP-Swap) instruction builders. Everything is parameterised by the program id
//! from the environment manifest; nothing here is tied to Raydium's mainnet or devnet ids.

use solana_program::hash::hash;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program, sysvar,
};
use transfer_hook_sdk::CpmmSwapAccounts;

const AMM_CONFIG_SEED: &[u8] = b"amm_config";
const AUTH_SEED: &[u8] = b"vault_and_lp_mint_auth_seed";
const POOL_SEED: &[u8] = b"pool";
const POOL_VAULT_SEED: &[u8] = b"pool_vault";
const POOL_LP_MINT_SEED: &[u8] = b"pool_lp_mint";
const OBSERVATION_SEED: &[u8] = b"observation";
const SUPPORT_MINT_SEED: &[u8] = b"support_mint";

pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub(crate) fn anchor_discriminator(name: &str) -> [u8; 8] {
    hash(format!("global:{name}").as_bytes()).to_bytes()[..8]
        .try_into()
        .expect("eight bytes")
}

#[derive(Clone, Copy, Debug)]
pub struct Cpmm {
    pub program_id: Pubkey,
    /// Wrapped-SOL token account that receives the pool-creation fee.
    pub fee_receiver: Pubkey,
}

#[derive(Clone, Copy, Debug)]
pub struct CpmmPool {
    pub amm_config: Pubkey,
    pub pool_state: Pubkey,
    pub mint_0: Pubkey,
    pub mint_1: Pubkey,
    pub vault_0: Pubkey,
    pub vault_1: Pubkey,
    pub observation: Pubkey,
    pub lp_mint: Pubkey,
    pub authority: Pubkey,
}

impl Cpmm {
    fn pda(&self, seeds: &[&[u8]]) -> Pubkey {
        Pubkey::find_program_address(seeds, &self.program_id).0
    }

    pub fn amm_config(&self, index: u16) -> Pubkey {
        self.pda(&[AMM_CONFIG_SEED, &index.to_be_bytes()])
    }

    pub fn authority(&self) -> Pubkey {
        self.pda(&[AUTH_SEED])
    }

    pub fn support_mint(&self, mint: &Pubkey) -> Pubkey {
        self.pda(&[SUPPORT_MINT_SEED, mint.as_ref()])
    }

    /// Pool addresses for two mints (the program requires `mint_0 < mint_1`).
    pub fn pool(&self, amm_config: Pubkey, mint_0: Pubkey, mint_1: Pubkey) -> CpmmPool {
        assert!(mint_0 < mint_1, "CPMM requires mint_0 < mint_1");
        let pool_state = self.pda(&[
            POOL_SEED,
            amm_config.as_ref(),
            mint_0.as_ref(),
            mint_1.as_ref(),
        ]);
        CpmmPool {
            amm_config,
            pool_state,
            mint_0,
            mint_1,
            vault_0: self.pda(&[POOL_VAULT_SEED, pool_state.as_ref(), mint_0.as_ref()]),
            vault_1: self.pda(&[POOL_VAULT_SEED, pool_state.as_ref(), mint_1.as_ref()]),
            observation: self.pda(&[OBSERVATION_SEED, pool_state.as_ref()]),
            lp_mint: self.pda(&[POOL_LP_MINT_SEED, pool_state.as_ref()]),
            authority: self.authority(),
        }
    }

    /// Admin only (the integration build's admin is our deployer key).
    #[allow(clippy::too_many_arguments)]
    pub fn create_amm_config_instruction(
        &self,
        admin: &Pubkey,
        index: u16,
        trade_fee_rate: u64,
        protocol_fee_rate: u64,
        fund_fee_rate: u64,
        create_pool_fee: u64,
        creator_fee_rate: u64,
    ) -> Instruction {
        let mut data = anchor_discriminator("create_amm_config").to_vec();
        data.extend_from_slice(&index.to_le_bytes());
        for value in [
            trade_fee_rate,
            protocol_fee_rate,
            fund_fee_rate,
            create_pool_fee,
            creator_fee_rate,
        ] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(*admin, true),
                AccountMeta::new(self.amm_config(index), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        }
    }

    /// Admin only. Required before a Token-2022 mint with a TransferHook extension can be used.
    pub fn create_support_mint_instruction(&self, admin: &Pubkey, mint: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(*admin, true),
                AccountMeta::new_readonly(*mint, false),
                AccountMeta::new(self.support_mint(mint), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: anchor_discriminator("create_support_mint_associated").to_vec(),
        }
    }

    /// `initialize`: create the pool and seed it from the creator's two token accounts.
    /// `support_mints` are passed as remaining accounts for mints with extensions.
    #[allow(clippy::too_many_arguments)]
    pub fn initialize_instruction(
        &self,
        creator: &Pubkey,
        pool: &CpmmPool,
        creator_token_0: &Pubkey,
        creator_token_1: &Pubkey,
        amount_0: u64,
        amount_1: u64,
        open_time: u64,
        support_mints: &[Pubkey],
    ) -> Instruction {
        let creator_lp_token = Pubkey::find_program_address(
            &[
                creator.as_ref(),
                spl_token::id().as_ref(),
                pool.lp_mint.as_ref(),
            ],
            &ASSOCIATED_TOKEN_PROGRAM_ID,
        )
        .0;
        let mut data = anchor_discriminator("initialize").to_vec();
        data.extend_from_slice(&amount_0.to_le_bytes());
        data.extend_from_slice(&amount_1.to_le_bytes());
        data.extend_from_slice(&open_time.to_le_bytes());
        let mut accounts = vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new_readonly(pool.amm_config, false),
            AccountMeta::new_readonly(pool.authority, false),
            AccountMeta::new(pool.pool_state, false),
            AccountMeta::new_readonly(pool.mint_0, false),
            AccountMeta::new_readonly(pool.mint_1, false),
            AccountMeta::new(pool.lp_mint, false),
            AccountMeta::new(*creator_token_0, false),
            AccountMeta::new(*creator_token_1, false),
            AccountMeta::new(creator_lp_token, false),
            AccountMeta::new(pool.vault_0, false),
            AccountMeta::new(pool.vault_1, false),
            AccountMeta::new(self.fee_receiver, false),
            AccountMeta::new(pool.observation, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ];
        accounts.extend(
            support_mints
                .iter()
                .map(|key| AccountMeta::new_readonly(*key, false)),
        );
        Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }
    }

    /// Fixed accounts of a swap where `input` is token 0 or token 1 of `pool`.
    pub fn swap_accounts(
        &self,
        payer: Pubkey,
        pool: &CpmmPool,
        input_is_token_0: bool,
        input_token_account: Pubkey,
        output_token_account: Pubkey,
    ) -> CpmmSwapAccounts {
        let (input_vault, output_vault, input_mint, output_mint) = if input_is_token_0 {
            (pool.vault_0, pool.vault_1, pool.mint_0, pool.mint_1)
        } else {
            (pool.vault_1, pool.vault_0, pool.mint_1, pool.mint_0)
        };
        CpmmSwapAccounts {
            payer,
            authority: pool.authority,
            amm_config: pool.amm_config,
            pool_state: pool.pool_state,
            input_token_account,
            output_token_account,
            input_vault,
            output_vault,
            input_token_program: spl_token_2022::id(),
            output_token_program: spl_token_2022::id(),
            input_token_mint: input_mint,
            output_token_mint: output_mint,
            observation_state: pool.observation,
        }
    }
}
