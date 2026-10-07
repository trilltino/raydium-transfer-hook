//! Atomic, attributed resolution of Token-2022 Transfer Hook accounts.
//!
//! The only resolver in this crate is built on the official
//! `spl-transfer-hook-interface` offchain helper and `spl-tlv-account-resolution`
//! lists. Resolution is pure: it works on a private scratch instruction and
//! returns a [`LegHook`]; nothing is appended to a caller's instruction until a
//! `frame_*` function does so after validating every leg.

use std::{cell::RefCell, future::Future};

use solana_program::{
    bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
    hash::hash,
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey,
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::account::ExtraAccountMeta;
use spl_token_2022::{
    extension::{transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions},
    state::Mint as Token2022Mint,
};
use spl_transfer_hook_interface::{
    get_extra_account_metas_address, instruction::TransferHookInstruction,
    offchain::add_extra_account_metas_for_execute,
};

use crate::error::{
    AuthorityExpectation, FetchError, HookChangeKind, HookProgramInvalidReason, LegError, LegRole,
    SplResolveError,
};

/// LoaderV4. Spelled as a constant to avoid the deprecated `solana_program::loader_v4` re-export.
pub const LOADER_V4_ID: Pubkey = pubkey!("LoaderV411111111111111111111111111111111111");

/// Raydium program ids that must never be accepted as a transfer-hook program.
/// These are public on-chain addresses (CPMM and CLMM, mainnet and devnet,
/// plus AMM v4); no Raydium source is vendored here.
pub const RAYDIUM_PROGRAM_IDS: [Pubkey; 5] = [
    pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C"),
    pubkey!("DRaycpLY18LhpbydsBWbVJtxpNv9oXPgjRSfpF2bWpYb"),
    pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"),
    pubkey!("DRayAUgENGQBKVaX8owNhgzkEDyoHTGVEGHVJT1E9pfH"),
    pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"),
];

const UPGRADEABLE_PROGRAM_TAG: u32 = 2;
const UPGRADEABLE_PROGRAM_DATA_TAG: u32 = 3;
const UPGRADEABLE_UNINITIALIZED_TAG: u32 = 0;
const UPGRADEABLE_PROGRAM_LEN: usize = 36;
const UPGRADEABLE_PROGRAM_DATA_META_LEN: usize = 45;
const LOADER_V4_HEADER_LEN: usize = 48;
const LOADER_V4_STATUS_OFFSET: usize = 40;
const TLV_ENTRY_HEADER_LEN: usize = 12;
const POD_SLICE_PREFIX_LEN: usize = 4;

/// An account as returned by the caller's fetcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplAccount {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// One token transfer inside a larger instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplTransferLeg {
    pub source: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
    pub amount: u64,
}

/// Which privilege escalations resolved extra accounts may carry.
///
/// The hook program and the validation list are always readonly non-signers.
/// Every other resolved account is rejected if it is a signer or writable,
/// unless explicitly allowed. A hook-controlled list must not be able to make a
/// wallet or pool account signer or writable.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivilegePolicy {
    /// Extra accounts that may be marked signer.
    pub allowed_signers: Vec<Pubkey>,
    /// Extra accounts that may be marked writable.
    pub allowed_writable: Vec<Pubkey>,
    /// Disable the check entirely. Only for hooks you fully trust.
    pub trust_hook_privileges: bool,
}

impl PrivilegePolicy {
    /// Reject every signer and writable extra account (the default).
    pub fn reject_all() -> Self {
        Self::default()
    }

    /// Allow the listed accounts to be writable.
    pub fn allowing_writable(keys: impl IntoIterator<Item = Pubkey>) -> Self {
        Self {
            allowed_writable: keys.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Accept whatever privileges the hook's list declares.
    pub fn trust_everything() -> Self {
        Self {
            trust_hook_privileges: true,
            ..Self::default()
        }
    }

    fn check(&self, meta: &AccountMeta) -> Result<(), SplResolveError> {
        if self.trust_hook_privileges {
            return Ok(());
        }
        if meta.is_signer && !self.allowed_signers.contains(&meta.pubkey) {
            return Err(SplResolveError::UnexpectedSigner {
                address: meta.pubkey,
            });
        }
        if meta.is_writable && !self.allowed_writable.contains(&meta.pubkey) {
            return Err(SplResolveError::UnexpectedWritable {
                address: meta.pubkey,
            });
        }
        Ok(())
    }
}

/// Knobs for [`resolve_leg`]. `Default` is strict on privileges and loaders but
/// does not pin a hook program: set `expected_hook_program` whenever the
/// caller knows which hook the mint should use.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ResolveOptions {
    /// If set, a mint whose hook is a different program (or has none) is rejected.
    pub expected_hook_program: Option<Pubkey>,
    /// Required state of the Transfer Hook extension authority.
    pub expected_hook_authority: AuthorityExpectation,
    pub privilege_policy: PrivilegePolicy,
    /// Loaders the hook program account may be owned by.
    pub allowed_loaders: Vec<Pubkey>,
    /// Reject mints that carry no hook.
    pub require_hook: bool,
    /// Re-read the mint, program, and list after resolving and fail if they
    /// changed. Narrows (but cannot close) the window in which the fetches
    /// observe different chain states; use [`HookFingerprint::verify_unchanged`]
    /// immediately before signing for the rest.
    pub recheck_consistency: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            expected_hook_program: None,
            expected_hook_authority: AuthorityExpectation::Any,
            privilege_policy: PrivilegePolicy::reject_all(),
            allowed_loaders: default_allowed_loaders(),
            require_hook: false,
            recheck_consistency: true,
        }
    }
}

impl ResolveOptions {
    pub fn with_expected_hook_program(mut self, program: Pubkey) -> Self {
        self.expected_hook_program = Some(program);
        self
    }

    pub fn with_expected_hook_authority(mut self, expectation: AuthorityExpectation) -> Self {
        self.expected_hook_authority = expectation;
        self
    }

    pub fn with_privilege_policy(mut self, policy: PrivilegePolicy) -> Self {
        self.privilege_policy = policy;
        self
    }

    pub fn with_allowed_loaders(mut self, loaders: Vec<Pubkey>) -> Self {
        self.allowed_loaders = loaders;
        self
    }

    pub fn requiring_hook(mut self) -> Self {
        self.require_hook = true;
        self
    }

    /// Map a platform policy decision onto resolution options.
    ///
    /// `ImmutableAtLaunch` requires the Token-2022 hook authority to be revoked,
    /// otherwise the hook program could be swapped after launch.
    pub fn from_policy_decision(decision: &hook_policy_model::PolicyDecision) -> Self {
        let revoke_authority =
            decision.authority_policy.is_immutable_at_launch() && decision.hook_program.is_some();
        Self {
            expected_hook_program: decision.hook_program.map(Pubkey::new_from_array),
            expected_hook_authority: if revoke_authority {
                AuthorityExpectation::Revoked
            } else {
                AuthorityExpectation::Any
            },
            require_hook: decision.hook_required,
            ..Self::default()
        }
    }
}

/// Loaders accepted by default: BPF loader v2, deprecated v1, upgradeable, and v4.
pub fn default_allowed_loaders() -> Vec<Pubkey> {
    vec![
        bpf_loader::id(),
        bpf_loader_deprecated::id(),
        bpf_loader_upgradeable::id(),
        LOADER_V4_ID,
    ]
}

/// Loader-specific state of the hook program, recorded for change detection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramFingerprint {
    /// BPF loader v1/v2: the code cannot change.
    Immutable,
    Upgradeable {
        program_data: Pubkey,
        slot: u64,
        upgrade_authority: Option<Pubkey>,
    },
    LoaderV4 {
        slot: u64,
        authority_or_next_version: Pubkey,
        status: u64,
    },
}

/// Everything about a hooked leg that must still be true when the transaction
/// is signed. Capture it with the leg ([`HookSlice::fingerprint`]) and check it
/// with [`HookFingerprint::verify_unchanged`] right before signing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookFingerprint {
    pub mint: Pubkey,
    pub hook_program: Pubkey,
    /// The Transfer Hook extension authority at resolution time.
    pub hook_authority: Option<Pubkey>,
    pub loader: Pubkey,
    pub program: ProgramFingerprint,
    pub validation_list: Pubkey,
    /// sha256 of the validation list account data.
    pub validation_list_hash: [u8; 32],
}

impl HookFingerprint {
    /// Re-read the chain and fail if the hook (or its list) is not exactly what
    /// was resolved. Returns a specific error for what changed.
    pub async fn verify_unchanged<F, Fut, E>(&self, fetch: F) -> Result<(), SplResolveError>
    where
        F: Fn(Pubkey) -> Fut,
        Fut: Future<Output = Result<Option<SplAccount>, E>>,
        E: Into<FetchError>,
    {
        let options = ResolveOptions {
            allowed_loaders: vec![self.loader],
            ..ResolveOptions::default()
        };
        let now = inspect_mint(&fetch, self.mint, &options).await?;
        let Some(now) = now else {
            return Err(SplResolveError::HookProgramChanged {
                program: self.hook_program,
                kind: HookChangeKind::HookRemoved,
            });
        };
        diff_fingerprint(self, &now.fingerprint)
    }
}

fn diff_fingerprint(
    before: &HookFingerprint,
    after: &HookFingerprint,
) -> Result<(), SplResolveError> {
    if before.hook_program != after.hook_program {
        return Err(SplResolveError::HookProgramChanged {
            program: before.hook_program,
            kind: HookChangeKind::MintRepointed(after.hook_program),
        });
    }
    if before.loader != after.loader || before.program != after.program {
        return Err(SplResolveError::HookProgramChanged {
            program: before.hook_program,
            kind: HookChangeKind::ProgramStateChanged,
        });
    }
    if before.hook_authority != after.hook_authority {
        return Err(SplResolveError::HookAuthorityViolation {
            expected: match before.hook_authority {
                Some(key) => AuthorityExpectation::Exactly(key),
                None => AuthorityExpectation::Revoked,
            },
            found: after.hook_authority,
        });
    }
    if before.validation_list_hash != after.validation_list_hash {
        return Err(SplResolveError::ValidationListChanged {
            address: before.validation_list,
            before: before.validation_list_hash,
            after: after.validation_list_hash,
        });
    }
    Ok(())
}

/// The resolved per-transfer hook tail: `extras.., hook_program, validation_list`.
///
/// There is no public constructor: the only way to obtain one is
/// [`resolve_leg`], so a framed instruction can only carry slices that were
/// derived from a real validation list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookSlice {
    metas: Vec<AccountMeta>,
    hook_program: Pubkey,
    validation_list: Pubkey,
    fingerprint: HookFingerprint,
}

impl HookSlice {
    pub(crate) fn new(
        metas: Vec<AccountMeta>,
        hook_program: Pubkey,
        validation_list: Pubkey,
        fingerprint: HookFingerprint,
    ) -> Self {
        Self {
            metas,
            hook_program,
            validation_list,
            fingerprint,
        }
    }

    /// The full slice in instruction order (N extras followed by the 2-account tail).
    pub fn metas(&self) -> &[AccountMeta] {
        &self.metas
    }

    /// The accounts resolved from the validation list, without the tail.
    pub fn extras(&self) -> &[AccountMeta] {
        &self.metas[..self.metas.len().saturating_sub(2)]
    }

    pub fn hook_program(&self) -> Pubkey {
        self.hook_program
    }

    pub fn validation_list(&self) -> Pubkey {
        self.validation_list
    }

    pub fn fingerprint(&self) -> &HookFingerprint {
        &self.fingerprint
    }

    pub fn len(&self) -> usize {
        self.metas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metas.is_empty()
    }
}

/// The resolution result for one transfer: either a hook slice or "no hook".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegHook {
    role: LegRole,
    transfer: SplTransferLeg,
    slice: Option<HookSlice>,
}

impl LegHook {
    pub(crate) fn new(role: LegRole, transfer: SplTransferLeg, slice: Option<HookSlice>) -> Self {
        Self {
            role,
            transfer,
            slice,
        }
    }

    pub fn role(&self) -> LegRole {
        self.role
    }

    pub fn transfer(&self) -> &SplTransferLeg {
        &self.transfer
    }

    pub fn slice(&self) -> Option<&HookSlice> {
        self.slice.as_ref()
    }

    pub fn is_hooked(&self) -> bool {
        self.slice.is_some()
    }

    /// Number of accounts this leg adds to the instruction (0 when unhooked).
    pub fn account_count(&self) -> usize {
        self.slice.as_ref().map_or(0, HookSlice::len)
    }

    pub fn fingerprint(&self) -> Option<&HookFingerprint> {
        self.slice.as_ref().map(HookSlice::fingerprint)
    }

    /// Check that this leg's hook is still exactly what was resolved.
    /// Unhooked legs only need the mint to still be unhooked.
    pub async fn verify_unchanged<F, Fut, E>(&self, fetch: F) -> Result<(), LegError>
    where
        F: Fn(Pubkey) -> Fut,
        Fut: Future<Output = Result<Option<SplAccount>, E>>,
        E: Into<FetchError>,
    {
        let attribute = |source| LegError {
            leg: self.role,
            mint: self.transfer.mint,
            source,
        };
        match &self.slice {
            Some(slice) => slice
                .fingerprint
                .verify_unchanged(fetch)
                .await
                .map_err(attribute),
            None => {
                let options = ResolveOptions::default();
                match inspect_mint(&fetch, self.transfer.mint, &options)
                    .await
                    .map_err(attribute)?
                {
                    None => Ok(()),
                    Some(now) => Err(attribute(SplResolveError::HookProgramChanged {
                        program: now.hook_program,
                        kind: HookChangeKind::MintRepointed(now.hook_program),
                    })),
                }
            }
        }
    }
}

/// Resolve every leg and fail atomically if any one fails.
pub async fn resolve_legs<F, Fut, E>(
    legs: &[(LegRole, SplTransferLeg)],
    options: &ResolveOptions,
    fetch: F,
) -> Result<Vec<LegHook>, LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let mut resolved = Vec::with_capacity(legs.len());
    for (role, leg) in legs {
        resolved.push(resolve_leg(*role, *leg, options, &fetch).await?);
    }
    Ok(resolved)
}

/// Resolve the hook tail of one transfer on a private scratch instruction.
///
/// The fetcher is called afresh for every account; nothing is cached between
/// calls. A mint without a hook (classic SPL Token, or Token-2022 with no
/// hook program set) yields an unhooked [`LegHook`] unless the options require one.
pub async fn resolve_leg<F, Fut, E>(
    role: LegRole,
    leg: SplTransferLeg,
    options: &ResolveOptions,
    fetch: F,
) -> Result<LegHook, LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let attribute = |source| LegError {
        leg: role,
        mint: leg.mint,
        source,
    };
    resolve_leg_inner(&leg, options, &fetch)
        .await
        .map(|slice| LegHook::new(role, leg, slice))
        .map_err(attribute)
}

async fn resolve_leg_inner<F, Fut, E>(
    leg: &SplTransferLeg,
    options: &ResolveOptions,
    fetch: &F,
) -> Result<Option<HookSlice>, SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let Some(inspection) = inspect_mint(fetch, leg.mint, options).await? else {
        if options.require_hook || options.expected_hook_program.is_some() {
            return Err(SplResolveError::HookRequired);
        }
        return Ok(None);
    };

    let mut scratch = Instruction {
        program_id: spl_token_2022::id(),
        accounts: vec![
            AccountMeta::new(leg.source, false),
            AccountMeta::new_readonly(leg.mint, false),
            AccountMeta::new(leg.destination, false),
            AccountMeta::new_readonly(leg.authority, false),
        ],
        data: Vec::new(),
    };
    let base_len = scratch.accounts.len();
    let fetch_failure: RefCell<Option<SplResolveError>> = RefCell::new(None);
    let list_address = inspection.list_address;
    let list_data = &inspection.list_data;

    let outcome = add_extra_account_metas_for_execute(
        &mut scratch,
        &inspection.hook_program,
        &leg.source,
        &leg.mint,
        &leg.destination,
        &leg.authority,
        leg.amount,
        |address| {
            let fetch_failure = &fetch_failure;
            async move {
                if address == list_address {
                    return Ok(Some(list_data.clone()));
                }
                match fetch_checked(fetch, address).await {
                    Ok(account) => Ok(account.map(|account| account.data)),
                    Err(error) => {
                        fetch_failure.borrow_mut().get_or_insert(error.clone());
                        Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
                    }
                }
            }
        },
    )
    .await;
    if let Err(error) = outcome {
        if let Some(recorded) = fetch_failure.into_inner() {
            return Err(recorded);
        }
        let code = error.downcast_ref::<ProgramError>().cloned();
        return Err(SplResolveError::ExtraAccountResolution {
            code,
            reason: error.to_string(),
        });
    }

    let appended: Vec<AccountMeta> = scratch.accounts.split_off(base_len);
    let tail_ok = appended.len() >= 2
        && appended[appended.len() - 2]
            == AccountMeta::new_readonly(inspection.hook_program, false)
        && appended[appended.len() - 1] == AccountMeta::new_readonly(list_address, false);
    if !tail_ok {
        return Err(SplResolveError::ExtraAccountResolution {
            code: None,
            reason: "resolver did not end with the hook program and validation list".into(),
        });
    }
    for extra in &appended[..appended.len() - 2] {
        options.privilege_policy.check(extra)?;
    }

    if options.recheck_consistency {
        let recheck_options = ResolveOptions {
            allowed_loaders: vec![inspection.fingerprint.loader],
            ..ResolveOptions::default()
        };
        match inspect_mint(fetch, leg.mint, &recheck_options).await? {
            Some(again) => diff_fingerprint(&inspection.fingerprint, &again.fingerprint)?,
            None => {
                return Err(SplResolveError::HookProgramChanged {
                    program: inspection.hook_program,
                    kind: HookChangeKind::HookRemoved,
                })
            }
        }
    }

    Ok(Some(HookSlice::new(
        appended,
        inspection.hook_program,
        list_address,
        inspection.fingerprint,
    )))
}

struct Inspection {
    hook_program: Pubkey,
    list_address: Pubkey,
    list_data: Vec<u8>,
    fingerprint: HookFingerprint,
}

async fn fetch_checked<F, Fut, E>(
    fetch: &F,
    address: Pubkey,
) -> Result<Option<SplAccount>, SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    match fetch(address).await {
        Ok(Some(account)) if account.key != address => Err(SplResolveError::AccountKeyMismatch {
            requested: address,
            returned: account.key,
        }),
        Ok(account) => Ok(account),
        Err(error) => Err(SplResolveError::AccountFetch {
            address,
            source: error.into(),
        }),
    }
}

/// Read and validate everything about a mint's hook except the extras.
/// `Ok(None)` means the mint has no hook.
async fn inspect_mint<F, Fut, E>(
    fetch: &F,
    mint: Pubkey,
    options: &ResolveOptions,
) -> Result<Option<Inspection>, SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let mint_account = fetch_checked(fetch, mint)
        .await?
        .ok_or(SplResolveError::MissingMint)?;
    let (hook_program, hook_authority) = if mint_account.owner == spl_token::id() {
        return Ok(None);
    } else if mint_account.owner == spl_token_2022::id() {
        let state = StateWithExtensions::<Token2022Mint>::unpack(&mint_account.data)
            .map_err(|_| SplResolveError::InvalidMintData)?;
        match state.get_extension::<TransferHook>() {
            Ok(extension) => (
                Option::<Pubkey>::from(extension.program_id),
                Option::<Pubkey>::from(extension.authority),
            ),
            Err(_) => (None, None),
        }
    } else {
        return Err(SplResolveError::InvalidMintOwner(mint_account.owner));
    };
    let Some(hook_program) = hook_program else {
        return Ok(None);
    };

    if let Some(expected) = options.expected_hook_program {
        if expected != hook_program {
            return Err(SplResolveError::UnexpectedHookProgram {
                expected,
                found: hook_program,
            });
        }
    }
    if let Some(reason) = invalid_hook_program_reason(&hook_program) {
        return Err(SplResolveError::HookProgramInvalid {
            program: hook_program,
            reason,
        });
    }
    match options.expected_hook_authority {
        AuthorityExpectation::Any => {}
        AuthorityExpectation::Exactly(expected) if hook_authority == Some(expected) => {}
        AuthorityExpectation::Revoked if hook_authority.is_none() => {}
        expected => {
            return Err(SplResolveError::HookAuthorityViolation {
                expected,
                found: hook_authority,
            })
        }
    }

    let (loader, program) = inspect_program(fetch, hook_program, options).await?;

    let list_address = get_extra_account_metas_address(&mint, &hook_program);
    let list_account = fetch_checked(fetch, list_address)
        .await?
        .ok_or(SplResolveError::MissingValidationList(list_address))?;
    if list_account.owner != hook_program {
        return Err(SplResolveError::InvalidValidationListOwner {
            address: list_address,
            owner: list_account.owner,
            expected: hook_program,
        });
    }
    check_validation_list(&list_address, &list_account.data)?;
    let validation_list_hash = hash(&list_account.data).to_bytes();

    Ok(Some(Inspection {
        hook_program,
        list_address,
        list_data: list_account.data,
        fingerprint: HookFingerprint {
            mint,
            hook_program,
            hook_authority,
            loader,
            program,
            validation_list: list_address,
            validation_list_hash,
        },
    }))
}

pub(crate) fn invalid_hook_program_reason(program: &Pubkey) -> Option<HookProgramInvalidReason> {
    if *program == Pubkey::default() {
        Some(HookProgramInvalidReason::Zero)
    } else if *program == spl_token_2022::id() {
        Some(HookProgramInvalidReason::Token2022Program)
    } else if *program == spl_token::id() {
        Some(HookProgramInvalidReason::SplTokenProgram)
    } else if RAYDIUM_PROGRAM_IDS.contains(program) {
        Some(HookProgramInvalidReason::RaydiumProgram)
    } else {
        None
    }
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(offset..offset.checked_add(8)?)?.try_into().ok()?,
    ))
}

fn read_key(data: &[u8], offset: usize) -> Option<Pubkey> {
    let bytes: [u8; 32] = data.get(offset..offset.checked_add(32)?)?.try_into().ok()?;
    Some(Pubkey::new_from_array(bytes))
}

async fn inspect_program<F, Fut, E>(
    fetch: &F,
    program: Pubkey,
    options: &ResolveOptions,
) -> Result<(Pubkey, ProgramFingerprint), SplResolveError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    let account = fetch_checked(fetch, program)
        .await?
        .ok_or(SplResolveError::MissingHookProgram)?;
    if !account.executable {
        return Err(SplResolveError::HookProgramNotExecutable);
    }
    let loader = account.owner;
    if !options.allowed_loaders.contains(&loader) {
        return Err(SplResolveError::HookProgramBadLoader { program, loader });
    }
    let malformed = || SplResolveError::HookProgramInvalid {
        program,
        reason: HookProgramInvalidReason::MalformedProgramAccount,
    };

    if loader == bpf_loader_upgradeable::id() {
        if read_u32(&account.data, 0) != Some(UPGRADEABLE_PROGRAM_TAG)
            || account.data.len() < UPGRADEABLE_PROGRAM_LEN
        {
            return Err(malformed());
        }
        let program_data = read_key(&account.data, 4).ok_or_else(malformed)?;
        let data_account = fetch_checked(fetch, program_data)
            .await?
            .ok_or(SplResolveError::HookProgramClosed { program })?;
        if data_account.owner != bpf_loader_upgradeable::id() {
            return Err(SplResolveError::HookProgramBadLoader {
                program,
                loader: data_account.owner,
            });
        }
        match read_u32(&data_account.data, 0) {
            Some(UPGRADEABLE_PROGRAM_DATA_TAG)
                if data_account.data.len() >= UPGRADEABLE_PROGRAM_DATA_META_LEN => {}
            Some(UPGRADEABLE_UNINITIALIZED_TAG) | None => {
                return Err(SplResolveError::HookProgramClosed { program })
            }
            Some(_) => return Err(malformed()),
        }
        let slot = read_u64(&data_account.data, 4).ok_or_else(malformed)?;
        let upgrade_authority = match data_account.data.get(12) {
            Some(0) => None,
            Some(1) => Some(read_key(&data_account.data, 13).ok_or_else(malformed)?),
            _ => return Err(malformed()),
        };
        Ok((
            loader,
            ProgramFingerprint::Upgradeable {
                program_data,
                slot,
                upgrade_authority,
            },
        ))
    } else if loader == LOADER_V4_ID {
        if account.data.len() < LOADER_V4_HEADER_LEN {
            return Err(malformed());
        }
        let slot = read_u64(&account.data, 0).ok_or_else(malformed)?;
        let authority_or_next_version = read_key(&account.data, 8).ok_or_else(malformed)?;
        let status = read_u64(&account.data, LOADER_V4_STATUS_OFFSET).ok_or_else(malformed)?;
        match status {
            0 => return Err(SplResolveError::HookProgramClosed { program }),
            1 | 2 => {}
            _ => return Err(malformed()),
        }
        Ok((
            loader,
            ProgramFingerprint::LoaderV4 {
                slot,
                authority_or_next_version,
                status,
            },
        ))
    } else {
        Ok((loader, ProgramFingerprint::Immutable))
    }
}

/// The 8-byte TLV discriminator of the SPL Execute instruction.
pub(crate) fn execute_discriminator() -> [u8; 8] {
    let packed = TransferHookInstruction::Execute { amount: 0 }.pack();
    packed[..8]
        .try_into()
        .expect("Execute packs a discriminator")
}

/// Structural check of an `ExtraAccountMetaList` account so a malformed list is
/// reported as such instead of as a generic fetch or resolution failure.
fn check_validation_list(address: &Pubkey, data: &[u8]) -> Result<(), SplResolveError> {
    let malformed = |reason: &str| SplResolveError::ValidationListMalformed {
        address: *address,
        reason: reason.to_string(),
    };
    let discriminator = execute_discriminator();
    let mut offset = 0usize;
    loop {
        let header_end = offset
            .checked_add(TLV_ENTRY_HEADER_LEN)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| malformed("no Execute entry in validation list"))?;
        let length = read_u32(data, offset + 8).ok_or_else(|| malformed("truncated TLV header"))?;
        let value_end = usize::try_from(length)
            .ok()
            .and_then(|length| header_end.checked_add(length))
            .filter(|end| *end <= data.len())
            .ok_or_else(|| malformed("TLV entry length exceeds account data"))?;
        if data[offset..offset + 8] == discriminator {
            let value = &data[header_end..value_end];
            let count =
                read_u32(value, 0).ok_or_else(|| malformed("missing ExtraAccountMeta count"))?;
            let needed = usize::try_from(count)
                .ok()
                .and_then(|count| count.checked_mul(std::mem::size_of::<ExtraAccountMeta>()))
                .and_then(|size| size.checked_add(POD_SLICE_PREFIX_LEN))
                .ok_or_else(|| malformed("ExtraAccountMeta count overflows"))?;
            if needed > value.len() {
                return Err(malformed("ExtraAccountMeta count exceeds entry length"));
            }
            return Ok(());
        }
        offset = value_end;
    }
}
