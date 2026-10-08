//! Instruction ABI of the pinned external Raydium CPMM / CLMM hook-support
//! forks: discriminators, fixed account order, and V1 / SwapV2 builders.
//!
//! These are *client-side* builders for the live Raydium instructions. They
//! exist so framing has a V1 baseline to be tested against and so goldens can
//! pin the fixed account order and flags. The order and flags are read from the
//! pinned upstream `Swap` / `SwapSingleV2` Anchor account structs
//! (`cargo xtask upstream fetch --hook --locked`).

use solana_program::{
    hash::hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

pub const CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
pub const CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR: [u8; 8] = [179, 135, 209, 217, 135, 75, 40, 58];
/// `swap_base_output` (exact output), unchanged. `SHA256("global:swap_base_output")[..8]`.
pub const CPMM_SWAP_BASE_OUTPUT_V1_DISCRIMINATOR: [u8; 8] = [55, 217, 98, 86, 163, 74, 180, 173];
/// `swap_base_output_v2`, the hook-aware exact-output swap. `SHA256("global:swap_base_output_v2")[..8]`.
pub const CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR: [u8; 8] = [29, 143, 223, 109, 3, 111, 151, 147];
pub const CLMM_SWAP_V2_DISCRIMINATOR: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];
pub const CLMM_SWAP_V3_DISCRIMINATOR: [u8; 8] = [240, 224, 38, 33, 176, 31, 241, 175];

/// CPMM `Swap` accounts: payer .. observation_state.
pub const CPMM_SWAP_FIXED_ACCOUNTS: usize = 13;
/// CLMM `SwapSingleV2` accounts: payer .. output_vault_mint.
pub const CLMM_SWAP_FIXED_ACCOUNTS: usize = 13;
/// `amount_in: u64, minimum_amount_out: u64` after the 8-byte discriminator.
pub const CPMM_SWAP_BASE_INPUT_V1_DATA_LEN: usize = 24;
/// `amount, other_amount_threshold: u64, sqrt_price_limit_x64: u128, is_base_input: bool`.
pub const CLMM_SWAP_V2_DATA_LEN: usize = 41;

/// Index of each CPMM fixed account.
pub mod cpmm_index {
    pub const PAYER: usize = 0;
    pub const AUTHORITY: usize = 1;
    pub const AMM_CONFIG: usize = 2;
    pub const POOL_STATE: usize = 3;
    pub const INPUT_TOKEN_ACCOUNT: usize = 4;
    pub const OUTPUT_TOKEN_ACCOUNT: usize = 5;
    pub const INPUT_VAULT: usize = 6;
    pub const OUTPUT_VAULT: usize = 7;
    pub const INPUT_TOKEN_PROGRAM: usize = 8;
    pub const OUTPUT_TOKEN_PROGRAM: usize = 9;
    pub const INPUT_TOKEN_MINT: usize = 10;
    pub const OUTPUT_TOKEN_MINT: usize = 11;
    pub const OBSERVATION_STATE: usize = 12;
}

/// Index of each CLMM fixed account.
pub mod clmm_index {
    pub const PAYER: usize = 0;
    pub const AMM_CONFIG: usize = 1;
    pub const POOL_STATE: usize = 2;
    pub const INPUT_TOKEN_ACCOUNT: usize = 3;
    pub const OUTPUT_TOKEN_ACCOUNT: usize = 4;
    pub const INPUT_VAULT: usize = 5;
    pub const OUTPUT_VAULT: usize = 6;
    pub const OBSERVATION_STATE: usize = 7;
    pub const TOKEN_PROGRAM: usize = 8;
    pub const TOKEN_PROGRAM_2022: usize = 9;
    pub const MEMO_PROGRAM: usize = 10;
    pub const INPUT_VAULT_MINT: usize = 11;
    pub const OUTPUT_VAULT_MINT: usize = 12;
}

/// Anchor instruction discriminator: `sha256("global:<name>")[..8]`.
pub fn anchor_instruction_discriminator(name: &str) -> [u8; 8] {
    let digest = hash(format!("global:{name}").as_bytes()).to_bytes();
    digest[..8]
        .try_into()
        .expect("sha256 has eight leading bytes")
}

/// Fixed accounts of CPMM `swap_base_input` / `swap_base_input_v2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpmmSwapAccounts {
    pub payer: Pubkey,
    pub authority: Pubkey,
    pub amm_config: Pubkey,
    pub pool_state: Pubkey,
    pub input_token_account: Pubkey,
    pub output_token_account: Pubkey,
    pub input_vault: Pubkey,
    pub output_vault: Pubkey,
    pub input_token_program: Pubkey,
    pub output_token_program: Pubkey,
    pub input_token_mint: Pubkey,
    pub output_token_mint: Pubkey,
    pub observation_state: Pubkey,
}

impl CpmmSwapAccounts {
    /// The 13 fixed metas in program order with the program's flags.
    pub fn to_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.payer, true),
            AccountMeta::new_readonly(self.authority, false),
            AccountMeta::new_readonly(self.amm_config, false),
            AccountMeta::new(self.pool_state, false),
            AccountMeta::new(self.input_token_account, false),
            AccountMeta::new(self.output_token_account, false),
            AccountMeta::new(self.input_vault, false),
            AccountMeta::new(self.output_vault, false),
            AccountMeta::new_readonly(self.input_token_program, false),
            AccountMeta::new_readonly(self.output_token_program, false),
            AccountMeta::new_readonly(self.input_token_mint, false),
            AccountMeta::new_readonly(self.output_token_mint, false),
            AccountMeta::new(self.observation_state, false),
        ]
    }
}

/// Build the unframed V1 `swap_base_input` instruction (no hook accounts).
/// Build the unframed V1 `swap_base_output`: spend at most `max_amount_in` to receive exactly
/// `amount_out`. The accounts are the same thirteen as `swap_base_input`.
pub fn build_cpmm_swap_base_output_v1(
    program_id: Pubkey,
    accounts: &CpmmSwapAccounts,
    max_amount_in: u64,
    amount_out: u64,
) -> Instruction {
    let mut data = Vec::with_capacity(CPMM_SWAP_BASE_INPUT_V1_DATA_LEN);
    data.extend_from_slice(&CPMM_SWAP_BASE_OUTPUT_V1_DISCRIMINATOR);
    data.extend_from_slice(&max_amount_in.to_le_bytes());
    data.extend_from_slice(&amount_out.to_le_bytes());
    Instruction {
        program_id,
        accounts: accounts.to_metas(),
        data,
    }
}

pub fn build_cpmm_swap_base_input_v1(
    program_id: Pubkey,
    accounts: &CpmmSwapAccounts,
    amount_in: u64,
    minimum_amount_out: u64,
) -> Instruction {
    let mut data = Vec::with_capacity(CPMM_SWAP_BASE_INPUT_V1_DATA_LEN);
    data.extend_from_slice(&CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&minimum_amount_out.to_le_bytes());
    Instruction {
        program_id,
        accounts: accounts.to_metas(),
        data,
    }
}

/// Fixed accounts of CLMM `swap_v2` / `swap_v3` (`SwapSingleV2`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClmmSwapAccounts {
    pub payer: Pubkey,
    pub amm_config: Pubkey,
    pub pool_state: Pubkey,
    pub input_token_account: Pubkey,
    pub output_token_account: Pubkey,
    pub input_vault: Pubkey,
    pub output_vault: Pubkey,
    pub observation_state: Pubkey,
    pub token_program: Pubkey,
    pub token_program_2022: Pubkey,
    pub memo_program: Pubkey,
    pub input_vault_mint: Pubkey,
    pub output_vault_mint: Pubkey,
}

impl ClmmSwapAccounts {
    /// The 13 fixed metas in program order with the program's flags.
    pub fn to_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.payer, true),
            AccountMeta::new_readonly(self.amm_config, false),
            AccountMeta::new(self.pool_state, false),
            AccountMeta::new(self.input_token_account, false),
            AccountMeta::new(self.output_token_account, false),
            AccountMeta::new(self.input_vault, false),
            AccountMeta::new(self.output_vault, false),
            AccountMeta::new(self.observation_state, false),
            AccountMeta::new_readonly(self.token_program, false),
            AccountMeta::new_readonly(self.token_program_2022, false),
            AccountMeta::new_readonly(self.memo_program, false),
            AccountMeta::new_readonly(self.input_vault_mint, false),
            AccountMeta::new_readonly(self.output_vault_mint, false),
        ]
    }
}

/// Arguments of CLMM `swap_v2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClmmSwapArgs {
    pub amount: u64,
    pub other_amount_threshold: u64,
    pub sqrt_price_limit_x64: u128,
    pub is_base_input: bool,
}

/// Build the unframed `swap_v2` instruction. Remaining accounts are the tick
/// arrays (writable) followed by the optional bitmap extension (marked writable
/// like the tick arrays; a superset of what the program needs).
pub fn build_clmm_swap_v2(
    program_id: Pubkey,
    accounts: &ClmmSwapAccounts,
    tick_arrays: &[Pubkey],
    bitmap_extension: Option<Pubkey>,
    args: ClmmSwapArgs,
) -> Instruction {
    let mut metas = accounts.to_metas();
    metas.extend(tick_arrays.iter().map(|key| AccountMeta::new(*key, false)));
    metas.extend(bitmap_extension.map(|key| AccountMeta::new(key, false)));
    let mut data = Vec::with_capacity(CLMM_SWAP_V2_DATA_LEN);
    data.extend_from_slice(&CLMM_SWAP_V2_DISCRIMINATOR);
    data.extend_from_slice(&args.amount.to_le_bytes());
    data.extend_from_slice(&args.other_amount_threshold.to_le_bytes());
    data.extend_from_slice(&args.sqrt_price_limit_x64.to_le_bytes());
    data.push(u8::from(args.is_base_input));
    Instruction {
        program_id,
        accounts: metas,
        data,
    }
}
