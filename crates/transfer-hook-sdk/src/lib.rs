#![forbid(unsafe_code)]

use std::{fmt, ops::Range};

use hook_policy_model::{AccountMeta, Pubkey, TransferContext};

const MINT_BASE_LEN: usize = 82;
const VALIDATION_LIST_HEADER_LEN: usize = 12;

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
