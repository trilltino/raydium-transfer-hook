//! Token-2022 helpers: mints with optional TransferHook and TransferFee extensions, and token
//! accounts.

use solana_program::program_pack::Pack;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_instruction,
};
use spl_token_2022::{
    extension::{
        transfer_fee::instruction as transfer_fee_instruction,
        transfer_hook::instruction as transfer_hook_instruction, ExtensionType,
    },
    instruction as token_instruction,
    state::{Account as TokenAccount, Mint},
};

/// The Token-2022 extensions a flow puts on a mint (and so, as account extensions, on its token
/// accounts).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MintFeatures {
    /// A TransferHook extension, with **no hook program yet**: the hook is switched on later (see
    /// the hook setup providers of `raydium-hook-driver`).
    pub hook: bool,
    /// A TransferFee extension charging this many basis points (0 for none).
    pub transfer_fee_bps: u16,
}

impl MintFeatures {
    pub fn plain() -> Self {
        Self::default()
    }

    fn mint_extensions(self) -> Vec<ExtensionType> {
        let mut list = Vec::new();
        if self.hook {
            list.push(ExtensionType::TransferHook);
        }
        if self.transfer_fee_bps > 0 {
            list.push(ExtensionType::TransferFeeConfig);
        }
        list
    }

    fn account_extensions(self) -> Vec<ExtensionType> {
        let mut list = Vec::new();
        if self.hook {
            list.push(ExtensionType::TransferHookAccount);
        }
        if self.transfer_fee_bps > 0 {
            list.push(ExtensionType::TransferFeeAmount);
        }
        list
    }
}

/// Instructions that create a Token-2022 mint with `decimals` and the extensions in `features`.
/// `authority` is the mint authority and the authority of each extension.
pub fn create_mint_instructions(
    payer: &Pubkey,
    mint: &Keypair,
    authority: &Pubkey,
    decimals: u8,
    features: MintFeatures,
) -> Vec<Instruction> {
    let len = ExtensionType::try_calculate_account_len::<Mint>(&features.mint_extensions())
        .expect("Token-2022 mint length");
    let mut instructions = vec![system_instruction::create_account(
        payer,
        &mint.pubkey(),
        Rent::default().minimum_balance(len),
        len as u64,
        &spl_token_2022::id(),
    )];
    if features.hook {
        instructions.push(
            transfer_hook_instruction::initialize(
                &spl_token_2022::id(),
                &mint.pubkey(),
                Some(*authority),
                None,
            )
            .expect("TransferHook initialize"),
        );
    }
    if features.transfer_fee_bps > 0 {
        instructions.push(
            transfer_fee_instruction::initialize_transfer_fee_config(
                &spl_token_2022::id(),
                &mint.pubkey(),
                Some(authority),
                Some(authority),
                features.transfer_fee_bps,
                u64::MAX,
            )
            .expect("TransferFeeConfig initialize"),
        );
    }
    instructions.push(
        token_instruction::initialize_mint2(
            &spl_token_2022::id(),
            &mint.pubkey(),
            authority,
            None,
            decimals,
        )
        .expect("initialize_mint2"),
    );
    instructions
}

/// Instructions that create a Token-2022 token account for a mint with `features` (the account
/// extensions the mint's extensions require).
pub fn create_token_account_instructions(
    payer: &Pubkey,
    account: &Keypair,
    mint: &Pubkey,
    owner: &Pubkey,
    features: MintFeatures,
) -> Vec<Instruction> {
    let len =
        ExtensionType::try_calculate_account_len::<TokenAccount>(&features.account_extensions())
            .expect("Token-2022 account length");
    vec![
        system_instruction::create_account(
            payer,
            &account.pubkey(),
            Rent::default().minimum_balance(len),
            len as u64,
            &spl_token_2022::id(),
        ),
        token_instruction::initialize_account3(
            &spl_token_2022::id(),
            &account.pubkey(),
            mint,
            owner,
        )
        .expect("initialize_account3"),
    ]
}

pub fn mint_to_instruction(
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    token_instruction::mint_to(
        &spl_token_2022::id(),
        mint,
        destination,
        authority,
        &[],
        amount,
    )
    .expect("mint_to")
}

/// A classic-token wrapped-SOL account at the address of `account` (CPMM's pool-creation fee
/// receiver is one).
pub fn create_wsol_account_instructions(
    payer: &Pubkey,
    account: &Keypair,
    owner: &Pubkey,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            payer,
            &account.pubkey(),
            Rent::default().minimum_balance(spl_token::state::Account::LEN),
            spl_token::state::Account::LEN as u64,
            &spl_token::id(),
        ),
        spl_token::instruction::initialize_account3(
            &spl_token::id(),
            &account.pubkey(),
            &spl_token::native_mint::id(),
            owner,
        )
        .expect("initialize wSOL account"),
    ]
}

/// An empty, rent-exempt wrapped-SOL (SPL Token) account owned by `owner`, as it would be after
/// [`create_wsol_account_instructions`]. For seeding a test chain's genesis with an account at an
/// address nobody holds the key to (the CPMM pool-creation fee receiver of a `localnet` build).
pub fn empty_wsol_account(owner: &Pubkey) -> solana_sdk::account::Account {
    let lamports = Rent::default().minimum_balance(spl_token::state::Account::LEN);
    let state = spl_token::state::Account {
        mint: spl_token::native_mint::id(),
        owner: *owner,
        amount: 0,
        state: spl_token::state::AccountState::Initialized,
        is_native: solana_program::program_option::COption::Some(lamports),
        ..Default::default()
    };
    let mut data = vec![0; spl_token::state::Account::LEN];
    spl_token::state::Account::pack(state, &mut data).expect("pack wSOL account");
    solana_sdk::account::Account {
        lamports,
        data,
        owner: spl_token::id(),
        executable: false,
        rent_epoch: 0,
    }
}

/// Token amount of a Token-2022 account's raw data.
pub fn token_amount(data: &[u8]) -> Option<u64> {
    use spl_token_2022::extension::StateWithExtensions;
    StateWithExtensions::<TokenAccount>::unpack(data)
        .ok()
        .map(|state| state.base.amount)
}
