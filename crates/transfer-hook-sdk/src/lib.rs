#![forbid(unsafe_code)]

use std::{fmt, future::Future, ops::Range};

use hook_policy_model::{AccountMeta, Pubkey, TransferContext};
use solana_program::{instruction::Instruction, pubkey::Pubkey as SolanaPubkey};
use spl_token_2022::{
    extension::{transfer_hook::get_program_id, StateWithExtensions},
    state::Mint as Token2022Mint,
};
use spl_transfer_hook_interface::{
    get_extra_account_metas_address, offchain::add_extra_account_metas_for_execute,
};

const MINT_BASE_LEN: usize = 82;
const VALIDATION_LIST_HEADER_LEN: usize = 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplAccount {
    pub key: SolanaPubkey,
    pub owner: SolanaPubkey,
    pub data: Vec<u8>,
    pub executable: bool,
}

#[derive(Debug)]
pub enum SplResolveError {
    AccountFetch(Box<dyn std::error::Error + Send + Sync>),
    InvalidMintOwner(SolanaPubkey),
    AccountKeyMismatch,
    MissingMint,
    InvalidMintData,
    MissingHookProgram,
    HookProgramNotExecutable,
    MissingValidationList,
    InvalidValidationListOwner,
}

impl fmt::Display for SplResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountFetch(error) => write!(f, "account fetch failed: {error}"),
            Self::InvalidMintOwner(owner) => {
                write!(f, "mint owner {owner} is not SPL Token or Token-2022")
            }
            Self::AccountKeyMismatch => f.write_str("fetched account key does not match request"),
            Self::MissingMint => f.write_str("mint account was not found"),
            Self::InvalidMintData => f.write_str("mint account data is invalid"),
            Self::MissingHookProgram => f.write_str("hook program account was not found"),
            Self::HookProgramNotExecutable => {
                f.write_str("mint transfer-hook program account is not executable")
            }
            Self::MissingValidationList => {
                f.write_str("hook validation ExtraAccountMetaList account was not found")
            }
            Self::InvalidValidationListOwner => {
                f.write_str("validation ExtraAccountMetaList is not owned by the hook program")
            }
        }
    }
}

impl std::error::Error for SplResolveError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplResolvedTransferAccounts {
    pub hook_program: SolanaPubkey,
    pub validation_list: SolanaPubkey,
    pub appended_accounts: Vec<solana_program::instruction::AccountMeta>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplTransferLeg {
    pub source: SolanaPubkey,
    pub mint: SolanaPubkey,
    pub destination: SolanaPubkey,
    pub authority: SolanaPubkey,
    pub amount: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplResolvedTransferLeg {
    pub transfer: SplTransferLeg,
    pub start: usize,
    pub end: usize,
    pub resolved: Option<SplResolvedTransferAccounts>,
}

/// Resolve and append the current SPL Transfer Hook account slice for one transfer.
/// The fetcher is called for each invocation; no result is retained between transfers.
pub async fn resolve_spl_transfer_hook_accounts<F, Fut>(
    instruction: &mut Instruction,
    source: SolanaPubkey,
    mint: SolanaPubkey,
    destination: SolanaPubkey,
    authority: SolanaPubkey,
    amount: u64,
    fetch_account: F,
) -> Result<Option<SplResolvedTransferAccounts>, SplResolveError>
where
    F: Fn(SolanaPubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, Box<dyn std::error::Error + Send + Sync>>>,
{
    let mint_account = fetch_account(mint)
        .await
        .map_err(SplResolveError::AccountFetch)?
        .ok_or(SplResolveError::MissingMint)?;
    if mint_account.key != mint {
        return Err(SplResolveError::AccountKeyMismatch);
    }
    let hook_program = if mint_account.owner == spl_token::id() {
        return Ok(None);
    } else if mint_account.owner == spl_token_2022::id() {
        let state = StateWithExtensions::<Token2022Mint>::unpack(&mint_account.data)
            .map_err(|_| SplResolveError::InvalidMintData)?;
        get_program_id(&state)
    } else {
        return Err(SplResolveError::InvalidMintOwner(mint_account.owner));
    };
    let Some(hook_program) = hook_program else {
        return Ok(None);
    };

    let hook_account = fetch_account(hook_program)
        .await
        .map_err(SplResolveError::AccountFetch)?
        .ok_or(SplResolveError::MissingHookProgram)?;
    if hook_account.key != hook_program {
        return Err(SplResolveError::AccountKeyMismatch);
    }
    if !hook_account.executable {
        return Err(SplResolveError::HookProgramNotExecutable);
    }

    let validation_list = get_extra_account_metas_address(&mint, &hook_program);
    let validation_account = fetch_account(validation_list)
        .await
        .map_err(SplResolveError::AccountFetch)?
        .ok_or(SplResolveError::MissingValidationList)?;
    if validation_account.key != validation_list {
        return Err(SplResolveError::AccountKeyMismatch);
    }
    if validation_account.owner != hook_program {
        return Err(SplResolveError::InvalidValidationListOwner);
    }
    let validation_data = validation_account.data;

    let original_account_count = instruction.accounts.len();
    let mut resolved_instruction = instruction.clone();
    add_extra_account_metas_for_execute(
        &mut resolved_instruction,
        &hook_program,
        &source,
        &mint,
        &destination,
        &authority,
        amount,
        |address| {
            let fetch_account = &fetch_account;
            let validation_data = &validation_data;
            async move {
                if address == validation_list {
                    Ok(Some(validation_data.clone()))
                } else {
                    fetch_account(address)
                        .await
                        .and_then(|account| {
                            account.map_or(Ok(None), |account| {
                                if account.key == address {
                                    Ok(Some(account.data))
                                } else {
                                    Err(Box::new(std::io::Error::other(
                                        "account fetch returned a different key",
                                    ))
                                        as Box<dyn std::error::Error + Send + Sync>)
                                }
                            })
                        })
                        .map_err(|error| {
                            Box::new(std::io::Error::other(error.to_string()))
                                as Box<dyn std::error::Error + Send + Sync>
                        })
                }
            }
        },
    )
    .await
    .map_err(SplResolveError::AccountFetch)?;

    let appended_accounts = resolved_instruction.accounts[original_account_count..].to_vec();
    instruction.accounts.extend_from_slice(&appended_accounts);
    Ok(Some(SplResolvedTransferAccounts {
        hook_program,
        validation_list,
        appended_accounts,
    }))
}

/// Resolve hook accounts independently for each transfer and record its unmerged account range.
pub async fn resolve_spl_transfer_hook_batch<F, Fut>(
    instruction: &mut Instruction,
    transfers: &[SplTransferLeg],
    fetch_account: F,
) -> Result<Vec<SplResolvedTransferLeg>, SplResolveError>
where
    F: Fn(SolanaPubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, Box<dyn std::error::Error + Send + Sync>>>,
{
    let mut results = Vec::with_capacity(transfers.len());
    for transfer in transfers {
        let start = instruction.accounts.len();
        let resolved = resolve_spl_transfer_hook_accounts(
            instruction,
            transfer.source,
            transfer.mint,
            transfer.destination,
            transfer.authority,
            transfer.amount,
            &fetch_account,
        )
        .await?;
        let end = instruction.accounts.len();
        results.push(SplResolvedTransferLeg {
            transfer: *transfer,
            start,
            end,
            resolved,
        });
    }
    Ok(results)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintAccount {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub data_len: usize,
    pub transfer_hook_program: Option<Pubkey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationListAccount {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub mint: Pubkey,
    pub data_len: usize,
    pub has_execute_discriminator: bool,
}

pub trait TransferHookAccountSource {
    fn validation_list_address(&self, hook_program: Pubkey, mint: Pubkey) -> Pubkey;

    fn fetch_mint(&mut self, mint: Pubkey) -> Result<Option<MintAccount>, SourceError>;

    fn fetch_validation_list(
        &mut self,
        address: Pubkey,
    ) -> Result<Option<ValidationListAccount>, SourceError>;

    fn resolve_extra_accounts(
        &mut self,
        validation_list: &ValidationListAccount,
        transfer: TransferContext,
    ) -> Result<Vec<AccountMeta>, SourceError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceError(pub String);

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SourceError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveError {
    Source(SourceError),
    InvalidTokenProgramConfiguration,
    InvalidTransferContext,
    MintNotFound(Pubkey),
    MintKeyMismatch,
    MintNotToken2022,
    HookExtensionOnClassicMint,
    InvalidMintData,
    InvalidHookProgram,
    ValidationListNotFound(Pubkey),
    ValidationListKeyMismatch,
    ValidationListOwnerMismatch,
    ValidationListMintMismatch,
    InvalidValidationListData,
    InvalidExtraAccount(Pubkey),
    InvalidResolvedAccounts,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "account source failed: {error}"),
            Self::InvalidTokenProgramConfiguration => {
                f.write_str("distinct SPL Token and Token-2022 program ids must be configured")
            }
            Self::InvalidTransferContext => {
                f.write_str("transfer source, mint, destination, and authority are required")
            }
            Self::MintNotFound(mint) => write!(f, "mint account {mint:?} was not found"),
            Self::MintKeyMismatch => f.write_str("fetched account does not match requested mint"),
            Self::MintNotToken2022 => {
                f.write_str("mint is not owned by the configured SPL Token or Token-2022 program")
            }
            Self::HookExtensionOnClassicMint => {
                f.write_str("classic SPL Token mint cannot contain a Transfer Hook extension")
            }
            Self::InvalidMintData => {
                f.write_str("mint account data is shorter than the base mint layout")
            }
            Self::InvalidHookProgram => {
                f.write_str("mint contains a zero transfer-hook program id")
            }
            Self::ValidationListNotFound(key) => {
                write!(f, "validation list {key:?} was not found")
            }
            Self::ValidationListKeyMismatch => {
                f.write_str("validation list does not match the derived address")
            }
            Self::ValidationListOwnerMismatch => {
                f.write_str("validation list is not owned by the mint's hook program")
            }
            Self::ValidationListMintMismatch => {
                f.write_str("validation list is configured for a different mint")
            }
            Self::InvalidValidationListData => f.write_str(
                "validation list data is truncated or has the wrong Execute discriminator",
            ),
            Self::InvalidExtraAccount(key) => {
                write!(f, "resolver returned an invalid extra account {key:?}")
            }
            Self::InvalidResolvedAccounts => f.write_str(
                "resolved hook program, validation list, and extra accounts are inconsistent",
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

impl From<SourceError> for ResolveError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTransferAccounts {
    pub hook_program: Option<Pubkey>,
    pub validation_list: Option<Pubkey>,
    pub extra_accounts: Vec<AccountMeta>,
}

impl ResolvedTransferAccounts {
    pub fn instruction_append_accounts(&self) -> Result<Vec<AccountMeta>, ResolveError> {
        let (Some(hook_program), Some(validation_list)) = (self.hook_program, self.validation_list)
        else {
            if self.hook_program.is_none()
                && self.validation_list.is_none()
                && self.extra_accounts.is_empty()
            {
                return Ok(Vec::new());
            }
            return Err(ResolveError::InvalidResolvedAccounts);
        };
        let mut accounts = self.extra_accounts.clone();
        accounts.push(AccountMeta::new(hook_program, false, false));
        accounts.push(AccountMeta::new(validation_list, false, false));
        Ok(accounts)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferAccountRange {
    pub transfer: TransferContext,
    pub hook_program: Option<Pubkey>,
    pub validation_list: Option<Pubkey>,
    pub accounts: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAccountBatch {
    pub accounts: Vec<AccountMeta>,
    pub transfers: Vec<TransferAccountRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferHookResolver {
    token_program: Pubkey,
    token_2022_program: Pubkey,
}

impl TransferHookResolver {
    pub const fn new(token_program: Pubkey, token_2022_program: Pubkey) -> Self {
        Self {
            token_program,
            token_2022_program,
        }
    }

    pub fn resolve_transfer_accounts<S: TransferHookAccountSource>(
        &self,
        source: &mut S,
        transfer: TransferContext,
    ) -> Result<ResolvedTransferAccounts, ResolveError> {
        if self.token_program == [0; 32]
            || self.token_2022_program == [0; 32]
            || self.token_program == self.token_2022_program
        {
            return Err(ResolveError::InvalidTokenProgramConfiguration);
        }
        if transfer.source == [0; 32]
            || transfer.mint == [0; 32]
            || transfer.destination == [0; 32]
            || transfer.authority == [0; 32]
        {
            return Err(ResolveError::InvalidTransferContext);
        }
        let mint = source
            .fetch_mint(transfer.mint)?
            .ok_or(ResolveError::MintNotFound(transfer.mint))?;
        if mint.key != transfer.mint {
            return Err(ResolveError::MintKeyMismatch);
        }
        if mint.data_len < MINT_BASE_LEN {
            return Err(ResolveError::InvalidMintData);
        }
        if mint.owner == self.token_program {
            if mint.transfer_hook_program.is_some() {
                return Err(ResolveError::HookExtensionOnClassicMint);
            }
            return Ok(ResolvedTransferAccounts {
                hook_program: None,
                validation_list: None,
                extra_accounts: Vec::new(),
            });
        }
        if mint.owner != self.token_2022_program {
            return Err(ResolveError::MintNotToken2022);
        }
        let Some(hook_program) = mint.transfer_hook_program else {
            return Ok(ResolvedTransferAccounts {
                hook_program: None,
                validation_list: None,
                extra_accounts: Vec::new(),
            });
        };
        if hook_program == [0; 32] || hook_program == self.token_2022_program {
            return Err(ResolveError::InvalidHookProgram);
        }

        let validation_key = source.validation_list_address(hook_program, transfer.mint);
        let validation_list = source
            .fetch_validation_list(validation_key)?
            .ok_or(ResolveError::ValidationListNotFound(validation_key))?;
        if validation_list.key != validation_key {
            return Err(ResolveError::ValidationListKeyMismatch);
        }
        if validation_list.owner != hook_program {
            return Err(ResolveError::ValidationListOwnerMismatch);
        }
        if validation_list.mint != transfer.mint {
            return Err(ResolveError::ValidationListMintMismatch);
        }
        if validation_list.data_len < VALIDATION_LIST_HEADER_LEN
            || !validation_list.has_execute_discriminator
        {
            return Err(ResolveError::InvalidValidationListData);
        }
        let extra_accounts = source.resolve_extra_accounts(&validation_list, transfer)?;
        if let Some(account) = extra_accounts.iter().find(|account| account.key == [0; 32]) {
            return Err(ResolveError::InvalidExtraAccount(account.key));
        }
        Ok(ResolvedTransferAccounts {
            hook_program: Some(hook_program),
            validation_list: Some(validation_key),
            extra_accounts,
        })
    }

    pub fn resolve_batch<S: TransferHookAccountSource>(
        &self,
        source: &mut S,
        transfers: &[TransferContext],
    ) -> Result<ResolvedAccountBatch, ResolveError> {
        let mut accounts = Vec::new();
        let mut ranges = Vec::with_capacity(transfers.len());
        for transfer in transfers {
            let start = accounts.len();
            let resolved = self.resolve_transfer_accounts(source, *transfer)?;
            let hook_program = resolved.hook_program;
            let validation_list = resolved.validation_list;
            accounts.extend(resolved.instruction_append_accounts()?);
            ranges.push(TransferAccountRange {
                transfer: *transfer,
                hook_program,
                validation_list,
                accounts: start..accounts.len(),
            });
        }
        Ok(ResolvedAccountBatch {
            accounts,
            transfers: ranges,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, error::Error};

    fn key(byte: u8) -> Pubkey {
        [byte; 32]
    }

    #[derive(Clone)]
    struct MockSource {
        mint: MintAccount,
        list: ValidationListAccount,
        resolved: Vec<AccountMeta>,
        fetch_mint_count: usize,
        fetch_list_count: usize,
        resolved_contexts: Vec<TransferContext>,
    }

    impl TransferHookAccountSource for MockSource {
        fn validation_list_address(&self, hook_program: Pubkey, mint: Pubkey) -> Pubkey {
            let mut bytes = [0; 32];
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = hook_program[index]
                    .wrapping_add(mint[index].wrapping_mul(17))
                    .wrapping_add(index as u8);
            }
            bytes
        }

        fn fetch_mint(&mut self, mint: Pubkey) -> Result<Option<MintAccount>, SourceError> {
            self.fetch_mint_count += 1;
            Ok((self.mint.key == mint).then(|| self.mint.clone()))
        }

        fn fetch_validation_list(
            &mut self,
            address: Pubkey,
        ) -> Result<Option<ValidationListAccount>, SourceError> {
            self.fetch_list_count += 1;
            Ok((self.list.key == address).then(|| self.list.clone()))
        }

        fn resolve_extra_accounts(
            &mut self,
            _validation_list: &ValidationListAccount,
            transfer: TransferContext,
        ) -> Result<Vec<AccountMeta>, SourceError> {
            self.resolved_contexts.push(transfer);
            Ok(self.resolved.clone())
        }
    }

    fn source(mint: Pubkey, hook_program: Option<Pubkey>) -> MockSource {
        let hook_program_key = hook_program.unwrap_or(key(7));
        let mut value = MockSource {
            mint: MintAccount {
                key: mint,
                owner: key(9),
                data_len: MINT_BASE_LEN,
                transfer_hook_program: hook_program,
            },
            list: ValidationListAccount {
                key: [0; 32],
                owner: hook_program_key,
                mint,
                data_len: VALIDATION_LIST_HEADER_LEN,
                has_execute_discriminator: true,
            },
            resolved: vec![AccountMeta::new(key(8), false, true)],
            fetch_mint_count: 0,
            fetch_list_count: 0,
            resolved_contexts: Vec::new(),
        };
        value.list.key = value.validation_list_address(hook_program_key, mint);
        value
    }

    fn transfer(mint: Pubkey, source: Pubkey, destination: Pubkey) -> TransferContext {
        TransferContext {
            source,
            mint,
            destination,
            authority: key(6),
            amount: 42,
        }
    }

    fn token_2022_mint_data(hook_program: SolanaPubkey) -> Vec<u8> {
        use solana_program::program_option::COption;
        use spl_token_2022::{
            extension::{
                transfer_hook::TransferHook, BaseStateWithExtensionsMut, ExtensionType,
                StateWithExtensionsMut,
            },
            state::Mint,
        };

        let size = ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook])
            .unwrap();
        let mut data = vec![0; size];
        let mut state = StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut data).unwrap();
        let extension = state.init_extension::<TransferHook>(true).unwrap();
        extension.program_id = Some(hook_program).try_into().unwrap();
        extension.authority = Some(SolanaPubkey::new_unique()).try_into().unwrap();
        state.base.mint_authority = COption::Some(SolanaPubkey::new_unique());
        state.base.decimals = 0;
        state.base.is_initialized = true;
        state.base.freeze_authority = COption::None;
        state.pack_base();
        state.init_account_type().unwrap();
        data
    }

    fn empty_validation_list() -> Vec<u8> {
        use spl_tlv_account_resolution::state::ExtraAccountMetaList;
        use spl_transfer_hook_interface::instruction::ExecuteInstruction;

        let mut data = vec![0; ExtraAccountMetaList::size_of(0).unwrap()];
        ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, &[]).unwrap();
        data
    }

    #[tokio::test]
    async fn spl_resolver_uses_official_validation_pda_and_account_order() {
        let hook_program = SolanaPubkey::new_unique();
        let mint = SolanaPubkey::new_unique();
        let source = SolanaPubkey::new_unique();
        let destination = SolanaPubkey::new_unique();
        let authority = SolanaPubkey::new_unique();
        let validation_list = get_extra_account_metas_address(&mint, &hook_program);
        let accounts = HashMap::from([
            (
                mint,
                SplAccount {
                    key: mint,
                    owner: spl_token_2022::id(),
                    data: token_2022_mint_data(hook_program),
                    executable: false,
                },
            ),
            (
                hook_program,
                SplAccount {
                    key: hook_program,
                    owner: SolanaPubkey::new_unique(),
                    data: Vec::new(),
                    executable: true,
                },
            ),
            (
                validation_list,
                SplAccount {
                    key: validation_list,
                    owner: hook_program,
                    data: empty_validation_list(),
                    executable: false,
                },
            ),
        ]);
        let mut instruction = Instruction {
            program_id: spl_token_2022::id(),
            accounts: vec![
                solana_program::instruction::AccountMeta::new(source, false),
                solana_program::instruction::AccountMeta::new_readonly(mint, false),
                solana_program::instruction::AccountMeta::new(destination, false),
                solana_program::instruction::AccountMeta::new_readonly(authority, true),
            ],
            data: Vec::new(),
        };
        let fetch = |address| {
            let account = accounts.get(&address).cloned();
            async move { Ok::<_, Box<dyn Error + Send + Sync>>(account) }
        };

        let result = resolve_spl_transfer_hook_accounts(
            &mut instruction,
            source,
            mint,
            destination,
            authority,
            42,
            fetch,
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(result.validation_list, validation_list);
        assert_eq!(
            result
                .appended_accounts
                .iter()
                .map(|meta| meta.pubkey)
                .collect::<Vec<_>>(),
            vec![hook_program, validation_list]
        );
    }

    #[tokio::test]
    async fn spl_resolver_rejects_wrong_validation_owner_without_mutating_instruction() {
        let hook_program = SolanaPubkey::new_unique();
        let mint = SolanaPubkey::new_unique();
        let source = SolanaPubkey::new_unique();
        let destination = SolanaPubkey::new_unique();
        let authority = SolanaPubkey::new_unique();
        let validation_list = get_extra_account_metas_address(&mint, &hook_program);
        let accounts = HashMap::from([
            (
                mint,
                SplAccount {
                    key: mint,
                    owner: spl_token_2022::id(),
                    data: token_2022_mint_data(hook_program),
                    executable: false,
                },
            ),
            (
                hook_program,
                SplAccount {
                    key: hook_program,
                    owner: SolanaPubkey::new_unique(),
                    data: Vec::new(),
                    executable: true,
                },
            ),
            (
                validation_list,
                SplAccount {
                    key: validation_list,
                    owner: SolanaPubkey::new_unique(),
                    data: empty_validation_list(),
                    executable: false,
                },
            ),
        ]);
        let mut instruction = Instruction {
            program_id: spl_token_2022::id(),
            accounts: vec![
                solana_program::instruction::AccountMeta::new(source, false),
                solana_program::instruction::AccountMeta::new_readonly(mint, false),
                solana_program::instruction::AccountMeta::new(destination, false),
                solana_program::instruction::AccountMeta::new_readonly(authority, true),
            ],
            data: Vec::new(),
        };
        let original = instruction.accounts.clone();
        let fetch = |address| {
            let account = accounts.get(&address).cloned();
            async move { Ok::<_, Box<dyn Error + Send + Sync>>(account) }
        };

        let result = resolve_spl_transfer_hook_accounts(
            &mut instruction,
            source,
            mint,
            destination,
            authority,
            42,
            fetch,
        )
        .await;

        assert!(matches!(
            result,
            Err(SplResolveError::InvalidValidationListOwner)
        ));
        assert_eq!(instruction.accounts, original);
    }

    #[tokio::test]
    async fn spl_resolver_leaves_classic_token_instruction_unchanged() {
        let mint = SolanaPubkey::new_unique();
        let source = SolanaPubkey::new_unique();
        let destination = SolanaPubkey::new_unique();
        let authority = SolanaPubkey::new_unique();
        let accounts = HashMap::from([(
            mint,
            SplAccount {
                key: mint,
                owner: spl_token::id(),
                data: Vec::new(),
                executable: false,
            },
        )]);
        let mut instruction = Instruction {
            program_id: spl_token_2022::id(),
            accounts: vec![
                solana_program::instruction::AccountMeta::new(source, false),
                solana_program::instruction::AccountMeta::new_readonly(mint, false),
                solana_program::instruction::AccountMeta::new(destination, false),
                solana_program::instruction::AccountMeta::new_readonly(authority, true),
            ],
            data: Vec::new(),
        };
        let original = instruction.accounts.clone();
        let fetch = |address| {
            let account = accounts.get(&address).cloned();
            async move { Ok::<_, Box<dyn Error + Send + Sync>>(account) }
        };

        let result = resolve_spl_transfer_hook_accounts(
            &mut instruction,
            source,
            mint,
            destination,
            authority,
            42,
            fetch,
        )
        .await
        .unwrap();

        assert_eq!(result, None);
        assert_eq!(instruction.accounts, original);
    }

    #[tokio::test]
    async fn spl_batch_keeps_two_hooked_transfer_slices_independent() {
        let hook_program = SolanaPubkey::new_unique();
        let mint = SolanaPubkey::new_unique();
        let validation_list = get_extra_account_metas_address(&mint, &hook_program);
        let source_a = SolanaPubkey::new_unique();
        let source_b = SolanaPubkey::new_unique();
        let destination_a = SolanaPubkey::new_unique();
        let destination_b = SolanaPubkey::new_unique();
        let authority = SolanaPubkey::new_unique();
        let accounts = HashMap::from([
            (
                mint,
                SplAccount {
                    key: mint,
                    owner: spl_token_2022::id(),
                    data: token_2022_mint_data(hook_program),
                    executable: false,
                },
            ),
            (
                hook_program,
                SplAccount {
                    key: hook_program,
                    owner: SolanaPubkey::new_unique(),
                    data: Vec::new(),
                    executable: true,
                },
            ),
            (
                validation_list,
                SplAccount {
                    key: validation_list,
                    owner: hook_program,
                    data: empty_validation_list(),
                    executable: false,
                },
            ),
        ]);
        let meta = solana_program::instruction::AccountMeta::new_readonly;
        let mut instruction = Instruction {
            program_id: spl_token_2022::id(),
            accounts: vec![
                solana_program::instruction::AccountMeta::new(source_a, false),
                meta(mint, false),
                solana_program::instruction::AccountMeta::new(destination_a, false),
                meta(authority, true),
                solana_program::instruction::AccountMeta::new(source_b, false),
                meta(mint, false),
                solana_program::instruction::AccountMeta::new(destination_b, false),
                meta(authority, true),
            ],
            data: Vec::new(),
        };
        let base_len = instruction.accounts.len();
        let transfers = [
            SplTransferLeg {
                source: source_a,
                mint,
                destination: destination_a,
                authority,
                amount: 1,
            },
            SplTransferLeg {
                source: source_b,
                mint,
                destination: destination_b,
                authority,
                amount: 2,
            },
        ];
        let fetch = |address| {
            let account = accounts.get(&address).cloned();
            async move { Ok::<_, Box<dyn Error + Send + Sync>>(account) }
        };

        let legs = resolve_spl_transfer_hook_batch(&mut instruction, &transfers, fetch)
            .await
            .unwrap();

        assert_eq!(legs.len(), 2);
        assert_eq!((legs[0].start, legs[0].end), (base_len, base_len + 2));
        assert_eq!((legs[1].start, legs[1].end), (base_len + 2, base_len + 4));
        assert_eq!(
            instruction.accounts[legs[0].start..legs[0].end],
            instruction.accounts[legs[1].start..legs[1].end]
        );
        assert_eq!(legs[0].transfer.amount, 1);
        assert_eq!(legs[1].transfer.amount, 2);
    }

    #[test]
    fn no_hook_returns_no_appended_accounts_and_skips_list_fetch() {
        let mint = key(1);
        let mut source = source(mint, None);
        let resolver = TransferHookResolver::new(key(8), key(9));
        let resolved = resolver
            .resolve_transfer_accounts(&mut source, transfer(mint, key(2), key(3)))
            .unwrap();

        assert!(resolved.instruction_append_accounts().unwrap().is_empty());
        assert_eq!(source.fetch_mint_count, 1);
        assert_eq!(source.fetch_list_count, 0);
    }

    #[test]
    fn classic_token_mint_cannot_claim_a_transfer_hook_extension() {
        let mint = key(1);
        let mut source = source(mint, Some(key(7)));
        source.mint.owner = key(8);
        let resolver = TransferHookResolver::new(key(8), key(9));

        assert_eq!(
            resolver.resolve_transfer_accounts(&mut source, transfer(mint, key(2), key(3))),
            Err(ResolveError::HookExtensionOnClassicMint)
        );
    }

    #[test]
    fn hooked_resolution_appends_extras_then_hook_program_and_validation_list() {
        let mint = key(1);
        let hook = key(7);
        let validation_key = {
            let source = source(mint, Some(hook));
            source.validation_list_address(hook, mint)
        };
        let mut source = source(mint, Some(hook));
        let resolver = TransferHookResolver::new(key(8), key(9));
        let resolved = resolver
            .resolve_transfer_accounts(&mut source, transfer(mint, key(2), key(3)))
            .unwrap();

        assert_eq!(
            resolved.instruction_append_accounts().unwrap(),
            vec![
                AccountMeta::new(key(8), false, true),
                AccountMeta::new(hook, false, false),
                AccountMeta::new(validation_key, false, false),
            ]
        );
    }

    #[test]
    fn each_transfer_is_resolved_freshly_with_its_own_context_and_account_range() {
        let mint_a = key(1);
        let mint_b = key(10);
        let hook_a = key(7);
        let hook_b = key(11);
        let mut source_a = source(mint_a, Some(hook_a));
        let source_b = source(mint_b, Some(hook_b));
        source_a.fetch_mint_count = 0;
        let mut source = MultiMintSource {
            sources: vec![source_a, source_b],
        };
        let resolver = TransferHookResolver::new(key(8), key(9));
        let transfers = [
            transfer(mint_a, key(2), key(3)),
            transfer(mint_b, key(4), key(5)),
        ];
        let batch = resolver.resolve_batch(&mut source, &transfers).unwrap();

        assert_eq!(batch.transfers.len(), 2);
        assert_eq!(batch.transfers[0].accounts, 0..3);
        assert_eq!(batch.transfers[1].accounts, 3..6);
        assert_eq!(batch.accounts[0].key, key(8));
        assert_eq!(batch.accounts[3].key, key(8));
        assert_eq!(source.sources[0].fetch_mint_count, 1);
        assert_eq!(source.sources[1].fetch_mint_count, 1);
    }

    struct MultiMintSource {
        sources: Vec<MockSource>,
    }

    impl TransferHookAccountSource for MultiMintSource {
        fn validation_list_address(&self, hook_program: Pubkey, mint: Pubkey) -> Pubkey {
            self.sources[0].validation_list_address(hook_program, mint)
        }

        fn fetch_mint(&mut self, mint: Pubkey) -> Result<Option<MintAccount>, SourceError> {
            for source in &mut self.sources {
                if source.mint.key == mint {
                    return source.fetch_mint(mint);
                }
            }
            Ok(None)
        }

        fn fetch_validation_list(
            &mut self,
            address: Pubkey,
        ) -> Result<Option<ValidationListAccount>, SourceError> {
            for source in &mut self.sources {
                if source.list.key == address {
                    return source.fetch_validation_list(address);
                }
            }
            Ok(None)
        }

        fn resolve_extra_accounts(
            &mut self,
            validation_list: &ValidationListAccount,
            transfer: TransferContext,
        ) -> Result<Vec<AccountMeta>, SourceError> {
            for source in &mut self.sources {
                if source.list.key == validation_list.key {
                    return source.resolve_extra_accounts(validation_list, transfer);
                }
            }
            Err(SourceError("unrecognized validation list".into()))
        }
    }

    #[test]
    fn rejects_wrong_owner_and_malformed_validation_data() {
        let mint = key(1);
        let mut wrong_owner_source = source(mint, Some(key(7)));
        wrong_owner_source.mint.owner = key(10);
        let resolver = TransferHookResolver::new(key(8), key(9));
        assert_eq!(
            resolver
                .resolve_transfer_accounts(&mut wrong_owner_source, transfer(mint, key(2), key(3))),
            Err(ResolveError::MintNotToken2022)
        );

        let mut source = source(mint, Some(key(7)));
        source.list.has_execute_discriminator = false;
        assert_eq!(
            resolver.resolve_transfer_accounts(&mut source, transfer(mint, key(2), key(3))),
            Err(ResolveError::InvalidValidationListData)
        );
    }
}
