//! Raydium CLMM instruction builders, parameterised by the environment's program id.
//!
//! The pool is created at price 1 (tick 0) with one liquidity position over ticks [-300, 300]
//! at tick spacing 10. That range sits in two tick arrays (start -600 and 0), which a
//! `zero_for_one` swap consumes as `[A(0), A(-600)]`.

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program, sysvar,
};

use crate::cpmm::{anchor_discriminator, ASSOCIATED_TOKEN_PROGRAM_ID};

const AMM_CONFIG_SEED: &[u8] = b"amm_config";
const POOL_SEED: &[u8] = b"pool";
const POOL_VAULT_SEED: &[u8] = b"pool_vault";
const OBSERVATION_SEED: &[u8] = b"observation";
const BITMAP_SEED: &[u8] = b"pool_tick_array_bitmap_extension";
const TICK_ARRAY_SEED: &[u8] = b"tick_array";
const POSITION_SEED: &[u8] = b"position";
const SUPPORT_MINT_SEED: &[u8] = b"support_mint";

pub const MEMO_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
pub const TICK_SPACING: u16 = 10;
pub const TICK_LOWER: i32 = -300;
pub const TICK_UPPER: i32 = 300;
pub const LOWER_ARRAY_START: i32 = -600;
pub const UPPER_ARRAY_START: i32 = 0;

#[derive(Clone, Copy, Debug)]
pub struct Clmm {
    pub program_id: Pubkey,
}

#[derive(Clone, Copy, Debug)]
pub struct ClmmPool {
    pub amm_config: Pubkey,
    pub pool_state: Pubkey,
    pub mint_0: Pubkey,
    pub mint_1: Pubkey,
    pub vault_0: Pubkey,
    pub vault_1: Pubkey,
    pub observation: Pubkey,
    pub bitmap: Pubkey,
    /// `[A(0), A(-600)]`, the order a `zero_for_one` swap needs.
    pub tick_arrays: [Pubkey; 2],
}

/// A liquidity position opened by [`Clmm::open_position_instruction`]: the Token-2022 NFT that
/// owns it and the accounts the later instructions need.
#[derive(Clone, Copy, Debug)]
pub struct ClmmPosition {
    pub owner: Pubkey,
    pub nft_mint: Pubkey,
    pub nft_account: Pubkey,
    pub personal_position: Pubkey,
}

impl Clmm {
    fn pda(&self, seeds: &[&[u8]]) -> Pubkey {
        Pubkey::find_program_address(seeds, &self.program_id).0
    }

    pub fn amm_config(&self, index: u16) -> Pubkey {
        self.pda(&[AMM_CONFIG_SEED, &index.to_be_bytes()])
    }

    pub fn support_mint(&self, mint: &Pubkey) -> Pubkey {
        self.pda(&[SUPPORT_MINT_SEED, mint.as_ref()])
    }

    pub fn tick_array(&self, pool_state: &Pubkey, start_index: i32) -> Pubkey {
        self.pda(&[
            TICK_ARRAY_SEED,
            pool_state.as_ref(),
            &start_index.to_be_bytes(),
        ])
    }

    pub fn pool(&self, amm_config: Pubkey, mint_0: Pubkey, mint_1: Pubkey) -> ClmmPool {
        assert!(mint_0 < mint_1, "CLMM requires mint_0 < mint_1");
        let pool_state = self.pda(&[
            POOL_SEED,
            amm_config.as_ref(),
            mint_0.as_ref(),
            mint_1.as_ref(),
        ]);
        ClmmPool {
            amm_config,
            pool_state,
            mint_0,
            mint_1,
            vault_0: self.pda(&[POOL_VAULT_SEED, pool_state.as_ref(), mint_0.as_ref()]),
            vault_1: self.pda(&[POOL_VAULT_SEED, pool_state.as_ref(), mint_1.as_ref()]),
            observation: self.pda(&[OBSERVATION_SEED, pool_state.as_ref()]),
            bitmap: self.pda(&[BITMAP_SEED, pool_state.as_ref()]),
            tick_arrays: [
                self.tick_array(&pool_state, UPPER_ARRAY_START),
                self.tick_array(&pool_state, LOWER_ARRAY_START),
            ],
        }
    }

    /// Admin only.
    pub fn create_amm_config_instruction(
        &self,
        admin: &Pubkey,
        index: u16,
        tick_spacing: u16,
        trade_fee_rate: u32,
        protocol_fee_rate: u32,
        fund_fee_rate: u32,
    ) -> Instruction {
        let mut data = anchor_discriminator("create_amm_config").to_vec();
        data.extend_from_slice(&index.to_le_bytes());
        data.extend_from_slice(&tick_spacing.to_le_bytes());
        for value in [trade_fee_rate, protocol_fee_rate, fund_fee_rate] {
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

    /// `create_pool` at `sqrt_price_x64` (2^64 is price 1).
    pub fn create_pool_instruction(
        &self,
        creator: &Pubkey,
        pool: &ClmmPool,
        sqrt_price_x64: u128,
        open_time: u64,
        support_mints: &[Pubkey],
    ) -> Instruction {
        let mut data = anchor_discriminator("create_pool").to_vec();
        data.extend_from_slice(&sqrt_price_x64.to_le_bytes());
        data.extend_from_slice(&open_time.to_le_bytes());
        let mut accounts = vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new_readonly(pool.amm_config, false),
            AccountMeta::new(pool.pool_state, false),
            AccountMeta::new_readonly(pool.mint_0, false),
            AccountMeta::new_readonly(pool.mint_1, false),
            AccountMeta::new(pool.vault_0, false),
            AccountMeta::new(pool.vault_1, false),
            AccountMeta::new(pool.observation, false),
            AccountMeta::new(pool.bitmap, false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
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

    /// `open_position_with_token22_nft` over ticks [-300, 300]: creates both tick arrays, mints
    /// the position NFT to `owner`, and deposits both tokens from `provider_0` / `provider_1`.
    #[allow(clippy::too_many_arguments)]
    pub fn open_position_instruction(
        &self,
        payer: &Pubkey,
        owner: &Pubkey,
        position_nft_mint: &Pubkey,
        pool: &ClmmPool,
        provider_0: &Pubkey,
        provider_1: &Pubkey,
        liquidity: u128,
        amount_0_max: u64,
        amount_1_max: u64,
    ) -> Instruction {
        let nft_account = Pubkey::find_program_address(
            &[
                owner.as_ref(),
                spl_token_2022::id().as_ref(),
                position_nft_mint.as_ref(),
            ],
            &ASSOCIATED_TOKEN_PROGRAM_ID,
        )
        .0;
        let personal_position = self.pda(&[POSITION_SEED, position_nft_mint.as_ref()]);
        let mut data = anchor_discriminator("open_position_with_token22_nft").to_vec();
        data.extend_from_slice(&TICK_LOWER.to_le_bytes());
        data.extend_from_slice(&TICK_UPPER.to_le_bytes());
        data.extend_from_slice(&LOWER_ARRAY_START.to_le_bytes());
        data.extend_from_slice(&UPPER_ARRAY_START.to_le_bytes());
        data.extend_from_slice(&liquidity.to_le_bytes());
        data.extend_from_slice(&amount_0_max.to_le_bytes());
        data.extend_from_slice(&amount_1_max.to_le_bytes());
        data.push(0); // with_metadata = false
        data.push(0); // base_flag = None
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(*payer, true),
                AccountMeta::new_readonly(*owner, false),
                AccountMeta::new(*position_nft_mint, true),
                AccountMeta::new(nft_account, false),
                AccountMeta::new(pool.pool_state, false),
                // Deprecated `protocol_position`: an unconstrained account the program ignores.
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new(self.tick_array(&pool.pool_state, LOWER_ARRAY_START), false),
                AccountMeta::new(self.tick_array(&pool.pool_state, UPPER_ARRAY_START), false),
                AccountMeta::new(personal_position, false),
                AccountMeta::new(*provider_0, false),
                AccountMeta::new(*provider_1, false),
                AccountMeta::new(pool.vault_0, false),
                AccountMeta::new(pool.vault_1, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
                AccountMeta::new_readonly(pool.mint_0, false),
                AccountMeta::new_readonly(pool.mint_1, false),
            ],
            data,
        }
    }

    /// The accounts of the position whose NFT mint is `nft_mint`, owned by `owner`.
    pub fn position(&self, owner: &Pubkey, nft_mint: &Pubkey) -> ClmmPosition {
        ClmmPosition {
            owner: *owner,
            nft_mint: *nft_mint,
            nft_account: Pubkey::find_program_address(
                &[
                    owner.as_ref(),
                    spl_token_2022::id().as_ref(),
                    nft_mint.as_ref(),
                ],
                &ASSOCIATED_TOKEN_PROGRAM_ID,
            )
            .0,
            personal_position: self.pda(&[POSITION_SEED, nft_mint.as_ref()]),
        }
    }

    /// `increase_liquidity_v2` over the position's range, paid from `token_account_0` / `_1`
    /// (owned by the position's owner, who signs). Unframed: a pool with a hooked mint needs
    /// `increase_liquidity_v3` instead (see `transfer-hook-sdk`'s `ClmmLiquidityOp`).
    #[allow(clippy::too_many_arguments)]
    pub fn increase_liquidity_instruction(
        &self,
        pool: &ClmmPool,
        position: &ClmmPosition,
        token_account_0: &Pubkey,
        token_account_1: &Pubkey,
        liquidity: u128,
        amount_0_max: u64,
        amount_1_max: u64,
    ) -> Instruction {
        let mut data = anchor_discriminator("increase_liquidity_v2").to_vec();
        data.extend_from_slice(&liquidity.to_le_bytes());
        data.extend_from_slice(&amount_0_max.to_le_bytes());
        data.extend_from_slice(&amount_1_max.to_le_bytes());
        data.push(0); // base_flag = None
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(position.owner, true),
                AccountMeta::new_readonly(position.nft_account, false),
                AccountMeta::new(pool.pool_state, false),
                // Deprecated `protocol_position`: an unconstrained account the program ignores.
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new(position.personal_position, false),
                AccountMeta::new(self.tick_array(&pool.pool_state, LOWER_ARRAY_START), false),
                AccountMeta::new(self.tick_array(&pool.pool_state, UPPER_ARRAY_START), false),
                AccountMeta::new(*token_account_0, false),
                AccountMeta::new(*token_account_1, false),
                AccountMeta::new(pool.vault_0, false),
                AccountMeta::new(pool.vault_1, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
                AccountMeta::new_readonly(pool.mint_0, false),
                AccountMeta::new_readonly(pool.mint_1, false),
            ],
            data,
        }
    }

    /// `decrease_liquidity_v2`: takes `liquidity` out of the position and pays it, with the position's
    /// owed fees, to `recipient_0` / `_1`. With `liquidity == 0` it only collects the fees. Unframed:
    /// a pool with a hooked mint needs `decrease_liquidity_v3`.
    #[allow(clippy::too_many_arguments)]
    pub fn decrease_liquidity_instruction(
        &self,
        pool: &ClmmPool,
        position: &ClmmPosition,
        recipient_0: &Pubkey,
        recipient_1: &Pubkey,
        liquidity: u128,
        amount_0_min: u64,
        amount_1_min: u64,
    ) -> Instruction {
        let mut data = anchor_discriminator("decrease_liquidity_v2").to_vec();
        data.extend_from_slice(&liquidity.to_le_bytes());
        data.extend_from_slice(&amount_0_min.to_le_bytes());
        data.extend_from_slice(&amount_1_min.to_le_bytes());
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(position.owner, true),
                AccountMeta::new_readonly(position.nft_account, false),
                AccountMeta::new(position.personal_position, false),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new(pool.vault_0, false),
                AccountMeta::new(pool.vault_1, false),
                AccountMeta::new(self.tick_array(&pool.pool_state, LOWER_ARRAY_START), false),
                AccountMeta::new(self.tick_array(&pool.pool_state, UPPER_ARRAY_START), false),
                AccountMeta::new(*recipient_0, false),
                AccountMeta::new(*recipient_1, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
                AccountMeta::new_readonly(MEMO_PROGRAM_ID, false),
                AccountMeta::new_readonly(pool.mint_0, false),
                AccountMeta::new_readonly(pool.mint_1, false),
            ],
            data,
        }
    }

    /// `collect_protocol_fee` (or `collect_fund_fee` when `fund`): the admin sends what has accrued, up
    /// to the amounts requested, to `recipient_0` / `_1`. Unframed.
    #[allow(clippy::too_many_arguments)]
    pub fn collect_fee_instruction(
        &self,
        fund: bool,
        admin: &Pubkey,
        pool: &ClmmPool,
        recipient_0: &Pubkey,
        recipient_1: &Pubkey,
        amount_0_requested: u64,
        amount_1_requested: u64,
    ) -> Instruction {
        let mut data = anchor_discriminator(if fund {
            "collect_fund_fee"
        } else {
            "collect_protocol_fee"
        })
        .to_vec();
        data.extend_from_slice(&amount_0_requested.to_le_bytes());
        data.extend_from_slice(&amount_1_requested.to_le_bytes());
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(*admin, true),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new_readonly(pool.amm_config, false),
                AccountMeta::new(pool.vault_0, false),
                AccountMeta::new(pool.vault_1, false),
                AccountMeta::new_readonly(pool.mint_0, false),
                AccountMeta::new_readonly(pool.mint_1, false),
                AccountMeta::new(*recipient_0, false),
                AccountMeta::new(*recipient_1, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
            ],
            data,
        }
    }
}
