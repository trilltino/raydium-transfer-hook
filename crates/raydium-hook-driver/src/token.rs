//! Token-2022 helpers: mints with an optional TransferHook extension, and token accounts.

use solana_program::program_pack::Pack;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_instruction,
};
use spl_token_2022::{
    extension::{transfer_hook::instruction as transfer_hook_instruction, ExtensionType},
    instruction as token_instruction,
    state::{Account as TokenAccount, Mint},
};

/// Instructions that create a Token-2022 mint with `decimals`. When `with_hook_extension` is set
/// the TransferHook extension is initialised with `authority` as its authority and **no hook
/// program yet**; the hook is switched on later (see [`crate::hooks::HookSetup`]).
pub fn create_mint_instructions(
    payer: &Pubkey,
    mint: &Keypair,
    authority: &Pubkey,
    decimals: u8,
    with_hook_extension: bool,
) -> Vec<Instruction> {
    let extensions: &[ExtensionType] = if with_hook_extension {
        &[ExtensionType::TransferHook]
    } else {
        &[]
    };
    let len = ExtensionType::try_calculate_account_len::<Mint>(extensions)
        .expect("Token-2022 mint length");
    let mut instructions = vec![system_instruction::create_account(
        payer,
        &mint.pubkey(),
        Rent::default().minimum_balance(len),
        len as u64,
        &spl_token_2022::id(),
    )];
    if with_hook_extension {
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

/// Instructions that create a Token-2022 token account. `hooked` accounts carry the
/// TransferHookAccount extension (required for any mint with a TransferHook extension).
pub fn create_token_account_instructions(
    payer: &Pubkey,
    account: &Keypair,
    mint: &Pubkey,
    owner: &Pubkey,
    hooked: bool,
) -> Vec<Instruction> {
    let extensions: &[ExtensionType] = if hooked {
        &[ExtensionType::TransferHookAccount]
    } else {
        &[]
    };
    let len = ExtensionType::try_calculate_account_len::<TokenAccount>(extensions)
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

/// Token amount of a Token-2022 account's raw data.
pub fn token_amount(data: &[u8]) -> Option<u64> {
    use spl_token_2022::extension::StateWithExtensions;
    StateWithExtensions::<TokenAccount>::unpack(data)
        .ok()
        .map(|state| state.base.amount)
}
