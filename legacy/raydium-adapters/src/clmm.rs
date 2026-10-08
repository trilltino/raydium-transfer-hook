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
const OPERATION_SEED: &[u8] = b"operation";
const REWARD_VAULT_SEED: &[u8] = b"pool_reward_vault";

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

/// A limit order's accounts and the choices that fix them: its direction and tick decide which vault is
/// its input and which tick array holds its tick.
#[derive(Clone, Copy, Debug)]
pub struct ClmmLimitOrder {
    pub owner: Pubkey,
    pub nonce_index: u8,
    /// The PDA that counts the owner's orders under `nonce_index`.
    pub nonce: Pubkey,
    pub order: Pubkey,
    /// `true` deposits token_0 and is paid in token_1.
    pub zero_for_one: bool,
    pub tick_index: i32,
}

/// First tick of the tick array that holds `tick_index`: arrays cover `60 * TICK_SPACING` ticks.
pub fn tick_array_start(tick_index: i32) -> i32 {
    let span = 60 * i32::from(TICK_SPACING);
    tick_index.div_euclid(span) * span
}

/// The pool's current tick, from the raw account data of a `PoolState`.
pub fn pool_tick_current(pool_state_data: &[u8]) -> Option<i32> {
    // discriminator, bump, 7 keys, 2 decimals, tick spacing, liquidity, sqrt price, then the tick.
    const OFFSET: usize = 8 + 1 + 7 * 32 + 2 + 2 + 16 + 16;
    let bytes = pool_state_data.get(OFFSET..OFFSET + 4)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

/// How many orders a `LimitOrderNonce` account has counted (the next order's nonce), from its raw data.
pub fn limit_order_nonce_count(nonce_data: &[u8]) -> Option<u64> {
    // discriminator, owner, nonce index, then the counter.
    const OFFSET: usize = 8 + 32 + 1;
    let bytes = nonce_data.get(OFFSET..OFFSET + 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

/// `(total_amount, filled_amount)` of a `LimitOrderState`, from its raw data.
pub fn limit_order_amounts(order_data: &[u8]) -> Option<(u64, u64)> {
    // discriminator, pool, owner, tick, direction, order phase, then total and filled.
    const OFFSET: usize = 8 + 32 + 32 + 4 + 1 + 8;
    let total = u64::from_le_bytes(order_data.get(OFFSET..OFFSET + 8)?.try_into().ok()?);
    let filled = u64::from_le_bytes(order_data.get(OFFSET + 8..OFFSET + 16)?.try_into().ok()?);
    Some((total, filled))
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

    /// The accounts of the limit order that `owner` opens next under `nonce_index`, which has already
    /// opened `orders_so_far` (the nonce account's counter; 0 for a new nonce).
    pub fn limit_order(
        &self,
        owner: &Pubkey,
        nonce_index: u8,
        orders_so_far: u64,
        zero_for_one: bool,
        tick_index: i32,
    ) -> ClmmLimitOrder {
        let nonce =
            Pubkey::find_program_address(&[owner.as_ref(), &[nonce_index]], &self.program_id).0;
        let order = Pubkey::find_program_address(
            &[owner.as_ref(), nonce.as_ref(), &orders_so_far.to_be_bytes()],
            &self.program_id,
        )
        .0;
        ClmmLimitOrder {
            owner: *owner,
            nonce_index,
            nonce,
            order,
            zero_for_one,
            tick_index,
        }
    }

    /// `(input vault, output vault, input mint, output mint)` of an order.
    fn order_sides(pool: &ClmmPool, zero_for_one: bool) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
        if zero_for_one {
            (pool.vault_0, pool.vault_1, pool.mint_0, pool.mint_1)
        } else {
            (pool.vault_1, pool.vault_0, pool.mint_1, pool.mint_0)
        }
    }

    /// `open_limit_order` paid from `input_account`; `output_account` is only checked (not frozen).
    /// Unframed: a pool whose input token has a hook needs `open_limit_order_v2`.
    pub fn open_limit_order_instruction(
        &self,
        pool: &ClmmPool,
        order: &ClmmLimitOrder,
        amount: u64,
        input_account: &Pubkey,
        output_account: &Pubkey,
    ) -> Instruction {
        let (input_vault, output_vault, input_mint, output_mint) =
            Self::order_sides(pool, order.zero_for_one);
        let mut data = anchor_discriminator("open_limit_order").to_vec();
        data.push(order.nonce_index);
        data.push(u8::from(order.zero_for_one));
        data.extend_from_slice(&order.tick_index.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(order.owner, true),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new(
                    self.tick_array(&pool.pool_state, tick_array_start(order.tick_index)),
                    false,
                ),
                AccountMeta::new(order.nonce, false),
                AccountMeta::new(order.order, false),
                AccountMeta::new(*input_account, false),
                AccountMeta::new(*output_account, false),
                AccountMeta::new(input_vault, false),
                AccountMeta::new(output_vault, false),
                AccountMeta::new_readonly(input_mint, false),
                AccountMeta::new_readonly(output_mint, false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        }
    }

    /// `increase_limit_order`. Unframed (see `open_limit_order_instruction`).
    pub fn increase_limit_order_instruction(
        &self,
        pool: &ClmmPool,
        order: &ClmmLimitOrder,
        amount: u64,
        input_account: &Pubkey,
    ) -> Instruction {
        let (input_vault, _, input_mint, _) = Self::order_sides(pool, order.zero_for_one);
        let mut data = anchor_discriminator("increase_limit_order").to_vec();
        data.extend_from_slice(&amount.to_le_bytes());
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(order.owner, true),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new(
                    self.tick_array(&pool.pool_state, tick_array_start(order.tick_index)),
                    false,
                ),
                AccountMeta::new(order.order, false),
                AccountMeta::new(*input_account, false),
                AccountMeta::new(input_vault, false),
                AccountMeta::new_readonly(input_mint, false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
            ],
            data,
        }
    }

    /// `decrease_limit_order`: settles what has filled (paid to `output_account`), then takes `amount`
    /// of the unfilled part back to `input_account`. Unframed.
    pub fn decrease_limit_order_instruction(
        &self,
        pool: &ClmmPool,
        order: &ClmmLimitOrder,
        amount: u64,
        amount_min: u64,
        input_account: &Pubkey,
        output_account: &Pubkey,
    ) -> Instruction {
        let (input_vault, output_vault, input_mint, output_mint) =
            Self::order_sides(pool, order.zero_for_one);
        let mut data = anchor_discriminator("decrease_limit_order").to_vec();
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&amount_min.to_le_bytes());
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(order.owner, true),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new(
                    self.tick_array(&pool.pool_state, tick_array_start(order.tick_index)),
                    false,
                ),
                AccountMeta::new(order.order, false),
                AccountMeta::new(*input_account, false),
                AccountMeta::new(*output_account, false),
                AccountMeta::new(input_vault, false),
                AccountMeta::new(output_vault, false),
                AccountMeta::new_readonly(input_mint, false),
                AccountMeta::new_readonly(output_mint, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
            ],
            data,
        }
    }

    /// `settle_limit_order`: pays what has filled to `output_account`. The order's owner (or the
    /// program's limit-order admin) signs. Unframed.
    pub fn settle_limit_order_instruction(
        &self,
        pool: &ClmmPool,
        order: &ClmmLimitOrder,
        signer: &Pubkey,
        output_account: &Pubkey,
    ) -> Instruction {
        let (_, output_vault, _, output_mint) = Self::order_sides(pool, order.zero_for_one);
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(*signer, true),
                AccountMeta::new_readonly(pool.pool_state, false),
                AccountMeta::new_readonly(
                    self.tick_array(&pool.pool_state, tick_array_start(order.tick_index)),
                    false,
                ),
                AccountMeta::new(order.order, false),
                AccountMeta::new(*output_account, false),
                AccountMeta::new(output_vault, false),
                AccountMeta::new_readonly(output_mint, false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
            ],
            data: anchor_discriminator("settle_limit_order").to_vec(),
        }
    }

    /// `close_limit_order`: closes a fully settled and cancelled order and returns its rent to the
    /// owner. It moves no tokens.
    pub fn close_limit_order_instruction(&self, order: &ClmmLimitOrder) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(order.owner, true),
                AccountMeta::new(order.owner, false),
                AccountMeta::new(order.order, false),
            ],
            data: anchor_discriminator("close_limit_order").to_vec(),
        }
    }

    /// The operation-state PDA that reward instructions check the funder against.
    pub fn operation_state(&self) -> Pubkey {
        self.pda(&[OPERATION_SEED])
    }

    /// Admin only: creates the operation-state account (once per program).
    pub fn create_operation_account_instruction(&self, admin: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(*admin, true),
                AccountMeta::new(self.operation_state(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: anchor_discriminator("create_operation_account").to_vec(),
        }
    }

    /// The vault that holds one reward mint for a pool.
    pub fn reward_vault(&self, pool_state: &Pubkey, reward_mint: &Pubkey) -> Pubkey {
        self.pda(&[REWARD_VAULT_SEED, pool_state.as_ref(), reward_mint.as_ref()])
    }

    /// `initialize_reward`: starts reward emission of `reward_mint` on the pool and funds the whole period
    /// from `funder_token_account`. A mint with a Transfer Hook also needs its support record
    /// (`support_mint`, see [`Clmm::support_mint`]) as the first remaining account. Unframed: a hooked
    /// reward mint needs `initialize_reward_v2`.
    #[allow(clippy::too_many_arguments)]
    pub fn initialize_reward_instruction(
        &self,
        funder: &Pubkey,
        funder_token_account: &Pubkey,
        pool: &ClmmPool,
        reward_mint: &Pubkey,
        open_time: u64,
        end_time: u64,
        emissions_per_second_x64: u128,
        support_mint: Option<Pubkey>,
    ) -> Instruction {
        let mut data = anchor_discriminator("initialize_reward").to_vec();
        data.extend_from_slice(&open_time.to_le_bytes());
        data.extend_from_slice(&end_time.to_le_bytes());
        data.extend_from_slice(&emissions_per_second_x64.to_le_bytes());
        let mut accounts = vec![
            AccountMeta::new(*funder, true),
            AccountMeta::new(*funder_token_account, false),
            AccountMeta::new_readonly(pool.amm_config, false),
            AccountMeta::new(pool.pool_state, false),
            AccountMeta::new_readonly(self.operation_state(), false),
            AccountMeta::new_readonly(*reward_mint, false),
            AccountMeta::new(self.reward_vault(&pool.pool_state, reward_mint), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ];
        if let Some(record) = support_mint {
            accounts.push(AccountMeta::new_readonly(record, false));
        }
        Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }
    }

    /// `set_reward_params`: changes a reward's emission or extends its period. When the change needs a
    /// top-up, `top_up` is `(authority_token_account, reward_mint)` and the vault, the account and the mint
    /// follow as remaining accounts. Unframed: a hooked reward mint needs `set_reward_params_v2`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_reward_params_instruction(
        &self,
        authority: &Pubkey,
        pool: &ClmmPool,
        reward_index: u8,
        emissions_per_second_x64: u128,
        open_time: u64,
        end_time: u64,
        top_up: Option<(Pubkey, Pubkey)>,
    ) -> Instruction {
        let mut data = anchor_discriminator("set_reward_params").to_vec();
        data.push(reward_index);
        data.extend_from_slice(&emissions_per_second_x64.to_le_bytes());
        data.extend_from_slice(&open_time.to_le_bytes());
        data.extend_from_slice(&end_time.to_le_bytes());
        let mut accounts = vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(pool.amm_config, false),
            AccountMeta::new(pool.pool_state, false),
            AccountMeta::new_readonly(self.operation_state(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
        ];
        if let Some((authority_token_account, reward_mint)) = top_up {
            accounts.push(AccountMeta::new(
                self.reward_vault(&pool.pool_state, &reward_mint),
                false,
            ));
            accounts.push(AccountMeta::new(authority_token_account, false));
            accounts.push(AccountMeta::new_readonly(reward_mint, false));
        }
        Instruction {
            program_id: self.program_id,
            accounts,
            data,
        }
    }

    /// `collect_remaining_rewards`: after a reward period ends, the funder takes back what was never
    /// emitted. Unframed: a hooked reward mint needs `collect_remaining_rewards_v2`.
    pub fn collect_remaining_rewards_instruction(
        &self,
        funder: &Pubkey,
        funder_token_account: &Pubkey,
        pool: &ClmmPool,
        reward_mint: &Pubkey,
        reward_index: u8,
    ) -> Instruction {
        let mut data = anchor_discriminator("collect_remaining_rewards").to_vec();
        data.push(reward_index);
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(*funder, true),
                AccountMeta::new(*funder_token_account, false),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new(self.reward_vault(&pool.pool_state, reward_mint), false),
                AccountMeta::new_readonly(*reward_mint, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
                AccountMeta::new_readonly(MEMO_PROGRAM_ID, false),
            ],
            data,
        }
    }

    /// [`Clmm::decrease_liquidity_instruction`] that also pays the position's pending rewards: one
    /// `(reward vault, recipient account, reward mint)` group per initialised reward, in reward order.
    /// Unframed: hooked reward mints need `decrease_liquidity_v4`.
    #[allow(clippy::too_many_arguments)]
    pub fn decrease_liquidity_with_rewards_instruction(
        &self,
        pool: &ClmmPool,
        position: &ClmmPosition,
        recipient_0: &Pubkey,
        recipient_1: &Pubkey,
        liquidity: u128,
        amount_0_min: u64,
        amount_1_min: u64,
        rewards: &[(Pubkey, Pubkey, Pubkey)],
    ) -> Instruction {
        let mut instruction = self.decrease_liquidity_instruction(
            pool,
            position,
            recipient_0,
            recipient_1,
            liquidity,
            amount_0_min,
            amount_1_min,
        );
        for (vault, recipient, mint) in rewards {
            instruction.accounts.push(AccountMeta::new(*vault, false));
            instruction
                .accounts
                .push(AccountMeta::new(*recipient, false));
            instruction
                .accounts
                .push(AccountMeta::new_readonly(*mint, false));
        }
        instruction
    }
}
