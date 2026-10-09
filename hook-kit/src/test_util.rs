//! Builders for the on-chain accounts the unit tests read.

use solana_program::{
    account_info::AccountInfo, program_option::COption, program_pack::Pack, pubkey::Pubkey,
};
use spl_pod::optional_keys::OptionalNonZeroPubkey;
use spl_token_2022::{
    extension::{
        transfer_hook::{TransferHook, TransferHookAccount},
        BaseStateWithExtensionsMut, ExtensionType, StateWithExtensionsMut,
    },
    state::{Account, AccountState, Mint},
};

/// A Token-2022 mint carrying a TransferHook extension.
pub fn hooked_mint(
    extension_authority: Option<Pubkey>,
    hook_program: Option<Pubkey>,
    mint_authority: Option<Pubkey>,
    supply: u64,
) -> Vec<u8> {
    let len =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook]).unwrap();
    let mut data = vec![0u8; len];
    let mut state = StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut data).unwrap();
    let extension = state.init_extension::<TransferHook>(true).unwrap();
    extension.authority = OptionalNonZeroPubkey::try_from(extension_authority).unwrap();
    extension.program_id = OptionalNonZeroPubkey::try_from(hook_program).unwrap();
    state.base = Mint {
        mint_authority: mint_authority.map_or(COption::None, COption::Some),
        supply,
        decimals: 6,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    state.pack_base();
    state.init_account_type().unwrap();
    data
}

/// A Token-2022 mint with no extensions.
pub fn plain_mint() -> Vec<u8> {
    let mut data = vec![0u8; Mint::LEN];
    Mint {
        is_initialized: true,
        ..Mint::default()
    }
    .pack_into_slice(&mut data);
    data
}

/// A Token-2022 token account of `mint` with the TransferHookAccount extension.
pub fn hooked_account(mint: Pubkey, owner: Pubkey, amount: u64, transferring: bool) -> Vec<u8> {
    let len =
        ExtensionType::try_calculate_account_len::<Account>(&[ExtensionType::TransferHookAccount])
            .unwrap();
    let mut data = vec![0u8; len];
    let mut state = StateWithExtensionsMut::<Account>::unpack_uninitialized(&mut data).unwrap();
    let extension = state.init_extension::<TransferHookAccount>(true).unwrap();
    extension.transferring = transferring.into();
    state.base = Account {
        mint,
        owner,
        amount,
        state: AccountState::Initialized,
        ..Account::default()
    };
    state.pack_base();
    state.init_account_type().unwrap();
    data
}

/// Run `f` on an `AccountInfo` for `key` owned by `owner` holding `data`.
pub fn with_account<R>(
    key: &Pubkey,
    owner: &Pubkey,
    is_signer: bool,
    data: &mut [u8],
    f: impl FnOnce(&AccountInfo) -> R,
) -> R {
    let mut lamports = 1_000_000u64;
    let info = AccountInfo::new(key, is_signer, false, &mut lamports, data, owner, false, 0);
    f(&info)
}
