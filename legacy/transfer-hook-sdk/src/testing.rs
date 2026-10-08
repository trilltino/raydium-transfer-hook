//! Helpers for tests: real Token-2022 mint bytes, real `ExtraAccountMetaList`
//! accounts, and an in-memory account fetcher. Compiled for this crate's own
//! tests and for downstream tests behind the `test-utils` feature.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    future::{ready, Ready},
};

use solana_program::{bpf_loader, bpf_loader_upgradeable, program_option::COption, pubkey::Pubkey};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, state::ExtraAccountMetaList};
use spl_token_2022::{
    extension::{
        transfer_hook::TransferHook, BaseStateWithExtensionsMut, ExtensionType,
        StateWithExtensionsMut,
    },
    state::Mint,
};
use spl_transfer_hook_interface::{
    get_extra_account_metas_address, instruction::ExecuteInstruction,
};

use crate::{error::FetchError, resolve::SplAccount};

/// Serialized Token-2022 mint with a Transfer Hook extension.
pub fn token_2022_mint_data(hook_program: Option<Pubkey>, authority: Option<Pubkey>) -> Vec<u8> {
    let size = ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook])
        .expect("mint size");
    let mut data = vec![0; size];
    let mut state = StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut data)
        .expect("unpack uninitialized mint");
    let extension = state
        .init_extension::<TransferHook>(true)
        .expect("init transfer hook extension");
    extension.program_id = hook_program.try_into().expect("hook program");
    extension.authority = authority.try_into().expect("hook authority");
    state.base.mint_authority = COption::Some(Pubkey::new_unique());
    state.base.decimals = 0;
    state.base.is_initialized = true;
    state.base.freeze_authority = COption::None;
    state.pack_base();
    state.init_account_type().expect("account type");
    data
}

/// Serialized Token-2022 mint with no extensions at all.
pub fn token_2022_plain_mint_data() -> Vec<u8> {
    use solana_program::program_pack::Pack;
    let mut data = vec![0; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::Some(Pubkey::new_unique()),
            supply: 0,
            decimals: 0,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut data,
    )
    .expect("pack mint");
    data
}

/// Serialized classic SPL Token mint.
pub fn classic_mint_data() -> Vec<u8> {
    use solana_program::program_pack::Pack;
    let mut data = vec![0; spl_token::state::Mint::LEN];
    spl_token::state::Mint::pack(
        spl_token::state::Mint {
            mint_authority: COption::Some(Pubkey::new_unique()),
            supply: 0,
            decimals: 0,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut data,
    )
    .expect("pack mint");
    data
}

/// A real `ExtraAccountMetaList` account body for the Execute interface.
pub fn validation_list_data(metas: &[ExtraAccountMeta]) -> Vec<u8> {
    let mut data = vec![0; ExtraAccountMetaList::size_of(metas.len()).expect("list size")];
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, metas).expect("init list");
    data
}

/// An in-memory chain: a fixed set of accounts the resolver can fetch.
#[derive(Default)]
pub struct MemoryChain {
    pub accounts: HashMap<Pubkey, SplAccount>,
    pub failing: HashSet<Pubkey>,
    fetches: Cell<usize>,
}

impl MemoryChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: Pubkey, owner: Pubkey, data: Vec<u8>, executable: bool) {
        self.accounts.insert(
            key,
            SplAccount {
                key,
                owner,
                data,
                executable,
            },
        );
    }

    /// An executable program owned by BPF loader v2.
    pub fn add_program(&mut self, program: Pubkey) {
        self.insert(program, bpf_loader::id(), Vec::new(), true);
    }

    /// An upgradeable program and its ProgramData account.
    pub fn add_upgradeable_program(
        &mut self,
        program: Pubkey,
        program_data: Pubkey,
        slot: u64,
        upgrade_authority: Option<Pubkey>,
    ) {
        let mut program_account = 2u32.to_le_bytes().to_vec();
        program_account.extend_from_slice(program_data.as_ref());
        self.insert(program, bpf_loader_upgradeable::id(), program_account, true);
        let mut data_account = 3u32.to_le_bytes().to_vec();
        data_account.extend_from_slice(&slot.to_le_bytes());
        match upgrade_authority {
            Some(key) => {
                data_account.push(1);
                data_account.extend_from_slice(key.as_ref());
            }
            None => {
                data_account.push(0);
                data_account.extend_from_slice(&[0; 32]);
            }
        }
        data_account.extend_from_slice(&[0xAA; 16]);
        self.insert(
            program_data,
            bpf_loader_upgradeable::id(),
            data_account,
            false,
        );
    }

    /// A Token-2022 mint hooked to `hook_program`, with a real validation list
    /// built from `extras`, and the hook program registered under BPF loader v2.
    pub fn add_hooked_mint(
        &mut self,
        mint: Pubkey,
        hook_program: Pubkey,
        authority: Option<Pubkey>,
        extras: &[ExtraAccountMeta],
    ) {
        self.insert(
            mint,
            spl_token_2022::id(),
            token_2022_mint_data(Some(hook_program), authority),
            false,
        );
        self.add_program(hook_program);
        self.set_validation_list(mint, hook_program, validation_list_data(extras));
    }

    pub fn set_validation_list(&mut self, mint: Pubkey, hook_program: Pubkey, data: Vec<u8>) {
        let address = get_extra_account_metas_address(&mint, &hook_program);
        self.insert(address, hook_program, data, false);
    }

    pub fn add_classic_mint(&mut self, mint: Pubkey) {
        self.insert(mint, spl_token::id(), classic_mint_data(), false);
    }

    pub fn add_unhooked_token_2022_mint(&mut self, mint: Pubkey) {
        self.insert(
            mint,
            spl_token_2022::id(),
            token_2022_plain_mint_data(),
            false,
        );
    }

    /// Number of fetches made through [`MemoryChain::fetcher`] so far.
    pub fn fetch_count(&self) -> usize {
        self.fetches.get()
    }

    /// A fetcher closure suitable for `resolve_leg` and friends.
    pub fn fetcher(&self) -> impl Fn(Pubkey) -> Ready<Result<Option<SplAccount>, FetchError>> + '_ {
        move |address| {
            self.fetches.set(self.fetches.get() + 1);
            if self.failing.contains(&address) {
                return ready(Err(FetchError::new(format!("rpc failure for {address}"))));
            }
            ready(Ok(self.accounts.get(&address).cloned()))
        }
    }
}

struct NoopWaker;

impl std::task::Wake for NoopWaker {
    fn wake(self: std::sync::Arc<Self>) {}
}

/// Drive a future that never actually suspends (such as one over
/// [`MemoryChain::fetcher`]) to completion without an async runtime.
/// Panics if the future returns `Pending`.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::task::Waker::from(std::sync::Arc::new(NoopWaker));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(output) => output,
        std::task::Poll::Pending => panic!("block_on: future was not immediately ready"),
    }
}
