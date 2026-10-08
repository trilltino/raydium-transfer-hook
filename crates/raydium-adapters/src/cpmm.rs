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

    /// The permission record of `authority`, which `initialize_with_permission` requires of its payer.
    pub fn permission(&self, authority: &Pubkey) -> Pubkey {
        self.pda(&[b"permission", authority.as_ref()])
    }

    /// Admin only: create the permission record that lets `authority` create pools with
    /// `initialize_with_permission`, the only way to make a pool that accrues creator fees.
    pub fn create_permission_instruction(&self, admin: &Pubkey, authority: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(*admin, true),
                AccountMeta::new_readonly(*authority, false),
                AccountMeta::new(self.permission(authority), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: anchor_discriminator("create_permission_pda").to_vec(),
        }
    }

    /// `initialize_with_permission` (V1): like `initialize`, signed by a payer that has a permission
    /// record, and with a `creator_fee_on` choice (0 both tokens, 1 token 0 only, 2 token 1 only).
    /// The payer is the pool's creator, so creator fees accrue to it.
    #[allow(clippy::too_many_arguments)]
    pub fn initialize_with_permission_instruction(
        &self,
        payer: &Pubkey,
        pool: &CpmmPool,
        payer_token_0: &Pubkey,
        payer_token_1: &Pubkey,
        amount_0: u64,
        amount_1: u64,
        open_time: u64,
        creator_fee_on: u8,
        support_mints: &[Pubkey],
    ) -> Instruction {
        let payer_lp_token = Self::lp_token_account(payer, pool);
        let mut data = anchor_discriminator("initialize_with_permission").to_vec();
        data.extend_from_slice(&amount_0.to_le_bytes());
        data.extend_from_slice(&amount_1.to_le_bytes());
        data.extend_from_slice(&open_time.to_le_bytes());
        data.push(creator_fee_on);
        let mut accounts = vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*payer, false),
            AccountMeta::new_readonly(pool.amm_config, false),
            AccountMeta::new_readonly(pool.authority, false),
            AccountMeta::new(pool.pool_state, false),
            AccountMeta::new_readonly(pool.mint_0, false),
            AccountMeta::new_readonly(pool.mint_1, false),
            AccountMeta::new(pool.lp_mint, false),
            AccountMeta::new(*payer_token_0, false),
            AccountMeta::new(*payer_token_1, false),
            AccountMeta::new(payer_lp_token, false),
            AccountMeta::new(pool.vault_0, false),
            AccountMeta::new(pool.vault_1, false),
            AccountMeta::new(self.fee_receiver, false),
            AccountMeta::new(pool.observation, false),
            AccountMeta::new_readonly(self.permission(payer), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(system_program::id(), false),
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

    /// The creator's LP-token account: the associated token account of the classic token program.
    pub fn lp_token_account(owner: &Pubkey, pool: &CpmmPool) -> Pubkey {
        Pubkey::find_program_address(
            &[
                owner.as_ref(),
                spl_token::id().as_ref(),
                pool.lp_mint.as_ref(),
            ],
            &ASSOCIATED_TOKEN_PROGRAM_ID,
        )
        .0
    }

    /// `deposit`: add liquidity for `lp_token_amount` LP tokens, paying at most the two maximums.
    /// Unframed (V1); frame it with `frame_cpmm_pair_or_passthrough(CpmmPairOp::Deposit, ..)`.
    #[allow(clippy::too_many_arguments)]
    pub fn deposit_instruction(
        &self,
        owner: &Pubkey,
        pool: &CpmmPool,
        owner_lp_token: &Pubkey,
        token_0_account: &Pubkey,
        token_1_account: &Pubkey,
        lp_token_amount: u64,
        maximum_token_0_amount: u64,
        maximum_token_1_amount: u64,
    ) -> Instruction {
        let mut data = anchor_discriminator("deposit").to_vec();
        for amount in [
            lp_token_amount,
            maximum_token_0_amount,
            maximum_token_1_amount,
        ] {
            data.extend_from_slice(&amount.to_le_bytes());
        }
        Instruction {
            program_id: self.program_id,
            accounts: self.liquidity_accounts(
                owner,
                pool,
                owner_lp_token,
                token_0_account,
                token_1_account,
                false,
            ),
            data,
        }
    }

    /// `withdraw`: burn `lp_token_amount` LP tokens for at least the two minimums. Unframed (V1).
    #[allow(clippy::too_many_arguments)]
    pub fn withdraw_instruction(
        &self,
        owner: &Pubkey,
        pool: &CpmmPool,
        owner_lp_token: &Pubkey,
        token_0_account: &Pubkey,
        token_1_account: &Pubkey,
        lp_token_amount: u64,
        minimum_token_0_amount: u64,
        minimum_token_1_amount: u64,
    ) -> Instruction {
        let mut data = anchor_discriminator("withdraw").to_vec();
        for amount in [
            lp_token_amount,
            minimum_token_0_amount,
            minimum_token_1_amount,
        ] {
            data.extend_from_slice(&amount.to_le_bytes());
        }
        Instruction {
            program_id: self.program_id,
            accounts: self.liquidity_accounts(
                owner,
                pool,
                owner_lp_token,
                token_0_account,
                token_1_account,
                true,
            ),
            data,
        }
    }

    /// The accounts shared by `deposit` (13) and `withdraw` (14, with the memo program).
    fn liquidity_accounts(
        &self,
        owner: &Pubkey,
        pool: &CpmmPool,
        owner_lp_token: &Pubkey,
        token_0_account: &Pubkey,
        token_1_account: &Pubkey,
        with_memo: bool,
    ) -> Vec<AccountMeta> {
        let mut accounts = vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(pool.authority, false),
            AccountMeta::new(pool.pool_state, false),
            AccountMeta::new(*owner_lp_token, false),
            AccountMeta::new(*token_0_account, false),
            AccountMeta::new(*token_1_account, false),
            AccountMeta::new(pool.vault_0, false),
            AccountMeta::new(pool.vault_1, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(pool.mint_0, false),
            AccountMeta::new_readonly(pool.mint_1, false),
            AccountMeta::new(pool.lp_mint, false),
        ];
        if with_memo {
            accounts.push(AccountMeta::new_readonly(
                crate::clmm::MEMO_PROGRAM_ID,
                false,
            ));
        }
        accounts
    }

    /// `collect_protocol_fee` (or `collect_fund_fee` when `fund` is set): send up to the requested
    /// amounts of the accrued fees to the two recipient token accounts. `owner` must be the
    /// account the program accepts for that fee. Unframed (V1).
    #[allow(clippy::too_many_arguments)]
    pub fn collect_fee_instruction(
        &self,
        fund: bool,
        owner: &Pubkey,
        pool: &CpmmPool,
        recipient_token_0: &Pubkey,
        recipient_token_1: &Pubkey,
        amount_0_requested: u64,
        amount_1_requested: u64,
    ) -> Instruction {
        let name = if fund {
            "collect_fund_fee"
        } else {
            "collect_protocol_fee"
        };
        let mut data = anchor_discriminator(name).to_vec();
        data.extend_from_slice(&amount_0_requested.to_le_bytes());
        data.extend_from_slice(&amount_1_requested.to_le_bytes());
        Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new_readonly(*owner, true),
                AccountMeta::new_readonly(pool.authority, false),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new_readonly(pool.amm_config, false),
                AccountMeta::new(pool.vault_0, false),
                AccountMeta::new(pool.vault_1, false),
                AccountMeta::new_readonly(pool.mint_0, false),
                AccountMeta::new_readonly(pool.mint_1, false),
                AccountMeta::new(*recipient_token_0, false),
                AccountMeta::new(*recipient_token_1, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_token_2022::id(), false),
            ],
            data,
        }
    }

    /// The associated token account of `owner` for `mint`, under the Token-2022 program.
    pub fn associated_token_2022(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[owner.as_ref(), spl_token_2022::id().as_ref(), mint.as_ref()],
            &ASSOCIATED_TOKEN_PROGRAM_ID,
        )
        .0
    }

    /// `collect_creator_fee` (the pool creator signs), or `collect_creator_fee_permissionless`
    /// (anyone pays; the fee still goes to the creator). The creator's two associated token
    /// accounts are created by the program if they do not exist. Unframed (V1).
    pub fn collect_creator_fee_instruction(
        &self,
        permissionless: bool,
        payer: &Pubkey,
        creator: &Pubkey,
        pool: &CpmmPool,
    ) -> Instruction {
        let creator_token_0 = Self::associated_token_2022(creator, &pool.mint_0);
        let creator_token_1 = Self::associated_token_2022(creator, &pool.mint_1);
        let creator_fee_share = self.pda(&[
            b"creator_fee_share",
            creator.as_ref(),
            pool.amm_config.as_ref(),
        ]);
        let common = [
            AccountMeta::new(pool.vault_0, false),
            AccountMeta::new(pool.vault_1, false),
            AccountMeta::new_readonly(pool.mint_0, false),
            AccountMeta::new_readonly(pool.mint_1, false),
            AccountMeta::new(creator_token_0, false),
            AccountMeta::new(creator_token_1, false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ];
        let mut accounts = if permissionless {
            vec![
                AccountMeta::new(*payer, true),
                AccountMeta::new_readonly(*creator, false),
                AccountMeta::new_readonly(pool.authority, false),
                AccountMeta::new(pool.pool_state, false),
            ]
        } else {
            vec![
                AccountMeta::new(*creator, true),
                AccountMeta::new_readonly(pool.authority, false),
                AccountMeta::new(pool.pool_state, false),
                AccountMeta::new_readonly(pool.amm_config, false),
            ]
        };
        accounts.extend(common);
        if permissionless {
            accounts.push(AccountMeta::new_readonly(pool.amm_config, false));
        }
        accounts.push(AccountMeta::new_readonly(creator_fee_share, false));
        Instruction {
            program_id: self.program_id,
            accounts,
            data: anchor_discriminator(if permissionless {
                "collect_creator_fee_permissionless"
            } else {
                "collect_creator_fee"
            })
            .to_vec(),
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
