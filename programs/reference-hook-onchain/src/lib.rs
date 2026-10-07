#![deny(unsafe_code)]
//! Reference Token-2022 Transfer Hook program (max-transfer rule only).
//!
//! # What this program enforces
//!
//! A single rule template, `max-transfer-v1`: a hooked transfer of `amount > limit` is rejected
//! with [`HookError::TransferExceedsLimit`]. Nothing else is enforced. Allow/deny lists,
//! timelocks and platform-level policy exist only as models in `programs/reference-hook`.
//!
//! # Accounts per mint
//!
//! * Config PDA, seeds `["hook-config", mint]`, owned by this program, `256 + params_len` bytes.
//! * SPL validation list PDA, seeds `["extra-account-metas", mint]`, holding exactly one
//!   seeds-based `ExtraAccountMeta` (`Literal "hook-config"` + `AccountKey{index: 1}`). The
//!   list bytes are therefore identical for every mint and never need migration.
//!
//! Both are created by one atomic [`HookInstruction::InitializeHook`] so a mint can never end up
//! with a config but no list (or the reverse).
//!
//! # Config byte layout (little-endian, packed, fixed offsets)
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 8 | `b"HKCONFIG"` |
//! | 8 | 1 | version (`1`) |
//! | 9 | 1 | bump of the config PDA |
//! | 10 | 1 | bump of the validation-list PDA |
//! | 11 | 1 | authority mode ([`AuthorityMode`]) |
//! | 12 | 4 | template_version |
//! | 16 | 32 | mint |
//! | 48 | 32 | platform_config (reserved, must be all zero) |
//! | 80 | 32 | config_authority (mode 2 only, otherwise all zero) |
//! | 112 | 32 | template_id (`"max-transfer-v1"` zero padded) |
//! | 144 | 32 | config_hash = `sha256(template_id \|\| template_version_le \|\| params)` |
//! | 176 | 8 | flags (no flag is defined yet, must be zero) |
//! | 184 | 8 | config_seq (incremented by every mutation) |
//! | 192 | 2 | params_len (`<= 256`) |
//! | 194 | 62 | reserved (must be all zero) |
//! | 256 | n | params (`max-transfer-v1`: `u64` limit, `n == 8`, limit > 0) |
//!
//! Parsing is strict: `len == 256 + params_len`, the discriminator, version, reserved bytes and
//! hash must all match. See [`HookConfig::decode`].
//!
//! # Instructions (8-byte ASCII discriminator, then little-endian fields)
//!
//! * `InitializeHook` (`b"HKINIT01"`): `mode u8, template_version u32, flags u64,
//!   template_id [u8;32], config_authority [u8;32], params_len u16, params`.
//!   Accounts: `config (w), validation_list (w), mint, authority (s), payer (s, w), system`.
//! * `UpdateConfig` (`b"HKUPDT01"`): `expected_seq u64, template_version u32, flags u64,
//!   params_len u16, params`. Accounts: `config (w), mint, authority (s)`.
//! * `SetConfigAuthority` (`b"HKSETAU1"`): `new [u8;32]`. Accounts: `config (w), mint, authority (s)`.
//!   Valid only in mode 2. A zero `new` is the one-way transition to mode 3 (Immutable).
//! * `Execute` (SPL interface discriminator): exactly six accounts
//!   `source, mint, destination, owner, validation_list, config`.
//!
//! SPL `InitializeExtraAccountMetaList` / `UpdateExtraAccountMetaList` are NOT aliased. They carry
//! no authority mode, template or params, so any alias would have to invent a rule for the mint
//! (for example "unlimited"), which weakens the checks. They are rejected with
//! [`HookError::SplInterfaceUnsupported`]; use `InitializeHook`.
//!
//! # Authority modes
//!
//! | Mode | Name | Init signer | Update / SetConfigAuthority signer |
//! |---|---|---|---|
//! | 0 | ExtensionAuthority | live `TransferHook.authority` | live `TransferHook.authority` |
//! | 1 | MintAuthority | live mint authority | live mint authority |
//! | 2 | Explicit | live `TransferHook.authority` (consent) | stored `config_authority` |
//! | 3 | Immutable | live `TransferHook.authority` | nobody (`ConfigImmutable`) |
//! | 4+ | reserved (PlatformControlled) | `UnsupportedMode` | `UnsupportedMode` |
//!
//! A required authority that is `None` yields [`HookError::AuthorityUnavailable`]. The BPF
//! upgrade authority of this program is a third, out-of-band authority: it can replace the code
//! and therefore the rule for every mint, whatever the per-mint mode says.
//!
//! # Error codes (`ProgramError::Custom`)
//!
//! See [`HookError`]; codes start at `0x7001`.

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    hash::hashv,
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};
use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
};
use spl_token_2022::{
    extension::{
        transfer_hook::{get_program_id, TransferHook, TransferHookAccount},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::{Account as TokenAccount, Mint},
};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

pub const CONFIG_SEED: &[u8] = b"hook-config";
pub const VALIDATION_LIST_SEED: &[u8] = b"extra-account-metas";
pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"HKCONFIG";
pub const CONFIG_VERSION: u8 = 1;
pub const CONFIG_HEADER_LEN: usize = 256;
pub const MAX_PARAMS_LEN: usize = 256;

pub const INITIALIZE_HOOK_DISCRIMINATOR: [u8; 8] = *b"HKINIT01";
pub const UPDATE_CONFIG_DISCRIMINATOR: [u8; 8] = *b"HKUPDT01";
pub const SET_CONFIG_AUTHORITY_DISCRIMINATOR: [u8; 8] = *b"HKSETAU1";
/// SPL Transfer Hook interface `Execute` discriminator.
pub const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];
/// SPL `InitializeExtraAccountMetaList` discriminator (rejected, see crate docs).
pub const SPL_INITIALIZE_LIST_DISCRIMINATOR: [u8; 8] = [43, 34, 13, 49, 167, 88, 235, 235];
/// SPL `UpdateExtraAccountMetaList` discriminator (rejected, see crate docs).
pub const SPL_UPDATE_LIST_DISCRIMINATOR: [u8; 8] = [157, 105, 42, 146, 102, 85, 241, 174];

/// Template id of the only supported rule, zero padded to 32 bytes.
pub const TEMPLATE_MAX_TRANSFER_V1: [u8; 32] = padded_template_id(b"max-transfer-v1");
pub const MAX_TRANSFER_PARAMS_LEN: usize = 8;
pub const MAX_TRANSFER_TEMPLATE_VERSION: u32 = 1;
/// Bits that may be set in `flags`. No flag is defined yet.
pub const KNOWN_FLAGS_MASK: u64 = 0;

/// Exact length of the validation list: TLV header (8 + 4) + pod slice prefix (4) + one meta (35).
pub const VALIDATION_LIST_LEN: usize = 8 + 4 + 4 + 35;
const EXECUTE_ACCOUNT_COUNT: usize = 6;
const INITIALIZE_HOOK_FIXED_LEN: usize = 8 + 1 + 4 + 8 + 32 + 32 + 2;
const UPDATE_CONFIG_FIXED_LEN: usize = 8 + 8 + 4 + 8 + 2;

const fn padded_template_id(name: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < name.len() {
        out[i] = name[i];
        i += 1;
    }
    out
}

entrypoint!(process_instruction);

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/// Typed hook errors, surfaced as `ProgramError::Custom(code)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum HookError {
    /// Execute was called while the source or destination is not flagged `transferring`.
    NotDirectInvocation = 0x7001,
    /// The mint account is not owned by the Token-2022 program.
    MintOwnerNotToken2022 = 0x7002,
    /// The mint's TransferHook program id is not this program.
    MintHookProgramMismatch = 0x7003,
    /// The mint has no TransferHook extension.
    MintHookExtensionMissing = 0x7004,
    /// The config account is not the config PDA of the given mint.
    InvalidConfigPda = 0x7005,
    /// The config account is not owned by this program.
    InvalidConfigOwner = 0x7006,
    /// The config bytes are malformed (length, discriminator, reserved bytes, field values).
    InvalidConfigData = 0x7007,
    /// The config version is not supported (fail closed).
    UnsupportedVersion = 0x7008,
    /// The validation list is not the canonical list of the mint, or is malformed.
    InvalidValidationList = 0x7009,
    /// The resolved extra accounts do not match the validation list.
    AccountOrderMismatch = 0x700a,
    /// The transfer amount is above the configured limit.
    TransferExceedsLimit = 0x700b,
    /// The signer is not the authority required by the config's authority mode.
    AuthorityMismatch = 0x700c,
    /// The authority required by the mode is `None` on the mint.
    AuthorityUnavailable = 0x700d,
    /// The config or validation list already exists.
    AlreadyInitialized = 0x700e,
    /// The authority mode is reserved or unknown, or not valid for this instruction.
    UnsupportedMode = 0x700f,
    /// `params_len` is above 256.
    ParamsTooLarge = 0x7010,
    /// `config_hash` does not match the stored template and params.
    HashMismatch = 0x7011,
    /// `expected_seq` is not the current `config_seq`.
    StaleConfigSeq = 0x7012,
    /// Execute was invoked with a number of accounts other than six.
    WrongAccountCount = 0x7013,
    /// The config is in Immutable mode and cannot be changed.
    ConfigImmutable = 0x7014,
    /// Template params or flags are invalid for the template.
    InvalidParams = 0x7015,
    /// The template id is not known to this program.
    UnknownTemplate = 0x7016,
    /// SPL InitializeExtraAccountMetaList / UpdateExtraAccountMetaList are not accepted.
    SplInterfaceUnsupported = 0x7017,
}

impl HookError {
    pub const ALL: [HookError; 23] = [
        HookError::NotDirectInvocation,
        HookError::MintOwnerNotToken2022,
        HookError::MintHookProgramMismatch,
        HookError::MintHookExtensionMissing,
        HookError::InvalidConfigPda,
        HookError::InvalidConfigOwner,
        HookError::InvalidConfigData,
        HookError::UnsupportedVersion,
        HookError::InvalidValidationList,
        HookError::AccountOrderMismatch,
        HookError::TransferExceedsLimit,
        HookError::AuthorityMismatch,
        HookError::AuthorityUnavailable,
        HookError::AlreadyInitialized,
        HookError::UnsupportedMode,
        HookError::ParamsTooLarge,
        HookError::HashMismatch,
        HookError::StaleConfigSeq,
        HookError::WrongAccountCount,
        HookError::ConfigImmutable,
        HookError::InvalidParams,
        HookError::UnknownTemplate,
        HookError::SplInterfaceUnsupported,
    ];

    pub const fn code(self) -> u32 {
        self as u32
    }

    /// Decode a `ProgramError::Custom` code back into a hook error.
    pub fn from_code(code: u32) -> Option<HookError> {
        Self::ALL.into_iter().find(|error| error.code() == code)
    }

    pub const fn name(self) -> &'static str {
        match self {
            HookError::NotDirectInvocation => "NotDirectInvocation",
            HookError::MintOwnerNotToken2022 => "MintOwnerNotToken2022",
            HookError::MintHookProgramMismatch => "MintHookProgramMismatch",
            HookError::MintHookExtensionMissing => "MintHookExtensionMissing",
            HookError::InvalidConfigPda => "InvalidConfigPda",
            HookError::InvalidConfigOwner => "InvalidConfigOwner",
            HookError::InvalidConfigData => "InvalidConfigData",
            HookError::UnsupportedVersion => "UnsupportedVersion",
            HookError::InvalidValidationList => "InvalidValidationList",
            HookError::AccountOrderMismatch => "AccountOrderMismatch",
            HookError::TransferExceedsLimit => "TransferExceedsLimit",
            HookError::AuthorityMismatch => "AuthorityMismatch",
            HookError::AuthorityUnavailable => "AuthorityUnavailable",
            HookError::AlreadyInitialized => "AlreadyInitialized",
            HookError::UnsupportedMode => "UnsupportedMode",
            HookError::ParamsTooLarge => "ParamsTooLarge",
            HookError::HashMismatch => "HashMismatch",
            HookError::StaleConfigSeq => "StaleConfigSeq",
            HookError::WrongAccountCount => "WrongAccountCount",
            HookError::ConfigImmutable => "ConfigImmutable",
            HookError::InvalidParams => "InvalidParams",
            HookError::UnknownTemplate => "UnknownTemplate",
            HookError::SplInterfaceUnsupported => "SplInterfaceUnsupported",
        }
    }
}

impl core::fmt::Display for HookError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} (0x{:x})", self.name(), self.code())
    }
}

impl std::error::Error for HookError {}

impl From<HookError> for ProgramError {
    fn from(error: HookError) -> Self {
        ProgramError::Custom(error.code())
    }
}

// ---------------------------------------------------------------------------------------------
// Authority modes and config state
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthorityMode {
    ExtensionAuthority = 0,
    MintAuthority = 1,
    Explicit = 2,
    Immutable = 3,
}

impl AuthorityMode {
    /// Mode 4 (PlatformControlled) is reserved and, like every unknown value, unsupported.
    pub fn from_u8(value: u8) -> Result<Self, HookError> {
        match value {
            0 => Ok(AuthorityMode::ExtensionAuthority),
            1 => Ok(AuthorityMode::MintAuthority),
            2 => Ok(AuthorityMode::Explicit),
            3 => Ok(AuthorityMode::Immutable),
            _ => Err(HookError::UnsupportedMode),
        }
    }
}

/// `sha256(template_id || template_version_le || params)`.
pub fn compute_config_hash(
    template_id: &[u8; 32],
    template_version: u32,
    params: &[u8],
) -> [u8; 32] {
    hashv(&[template_id, &template_version.to_le_bytes(), params]).to_bytes()
}

/// Little-endian parameters of `max-transfer-v1`.
pub fn max_transfer_params(limit: u64) -> [u8; MAX_TRANSFER_PARAMS_LEN] {
    limit.to_le_bytes()
}

/// Decoded, validated config account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookConfig {
    pub version: u8,
    pub bump: u8,
    pub list_bump: u8,
    pub authority_mode: AuthorityMode,
    pub template_version: u32,
    pub mint: Pubkey,
    pub platform_config: Pubkey,
    pub config_authority: Pubkey,
    pub template_id: [u8; 32],
    pub config_hash: [u8; 32],
    pub flags: u64,
    pub config_seq: u64,
    params_len: u16,
    params: [u8; MAX_PARAMS_LEN],
}

impl HookConfig {
    pub fn params(&self) -> &[u8] {
        &self.params[..usize::from(self.params_len)]
    }

    pub fn account_len(&self) -> usize {
        CONFIG_HEADER_LEN + usize::from(self.params_len)
    }

    /// The limit of a `max-transfer-v1` config.
    pub fn max_transfer_limit(&self) -> Result<u64, HookError> {
        if self.template_id != TEMPLATE_MAX_TRANSFER_V1 {
            return Err(HookError::UnknownTemplate);
        }
        let bytes: [u8; MAX_TRANSFER_PARAMS_LEN] = self
            .params()
            .try_into()
            .map_err(|_| HookError::InvalidConfigData)?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Build a config for a new mint. `config_hash` is computed from the template and params.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bump: u8,
        list_bump: u8,
        authority_mode: AuthorityMode,
        template_id: [u8; 32],
        template_version: u32,
        mint: Pubkey,
        config_authority: Pubkey,
        flags: u64,
        params: &[u8],
    ) -> Result<Self, HookError> {
        if params.len() > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge);
        }
        let mut buffer = [0u8; MAX_PARAMS_LEN];
        buffer[..params.len()].copy_from_slice(params);
        Ok(HookConfig {
            version: CONFIG_VERSION,
            bump,
            list_bump,
            authority_mode,
            template_version,
            mint,
            platform_config: Pubkey::default(),
            config_authority,
            template_id,
            config_hash: compute_config_hash(&template_id, template_version, params),
            flags,
            config_seq: 0,
            params_len: u16::try_from(params.len()).map_err(|_| HookError::ParamsTooLarge)?,
            params: buffer,
        })
    }

    /// Strict parse of a config account's data. Never panics on arbitrary bytes.
    ///
    /// Checks, in order: header length, discriminator, version (fail closed), params length and
    /// total length, reserved bytes, authority mode, mode/authority consistency, reserved
    /// platform_config, and finally the config hash.
    pub fn decode(data: &[u8]) -> Result<Self, HookError> {
        if data.len() < CONFIG_HEADER_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(HookError::InvalidConfigData);
        }
        let version = data[8];
        if version != CONFIG_VERSION {
            return Err(HookError::UnsupportedVersion);
        }
        let params_len = u16::from_le_bytes([data[192], data[193]]);
        if usize::from(params_len) > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge);
        }
        if data.len() != CONFIG_HEADER_LEN + usize::from(params_len) {
            return Err(HookError::InvalidConfigData);
        }
        if data[194..CONFIG_HEADER_LEN].iter().any(|byte| *byte != 0) {
            return Err(HookError::InvalidConfigData);
        }
        let authority_mode = AuthorityMode::from_u8(data[11])?;
        let template_version = u32::from_le_bytes(array_at(data, 12)?);
        let mint = Pubkey::new_from_array(array_at(data, 16)?);
        let platform_config = Pubkey::new_from_array(array_at(data, 48)?);
        let config_authority = Pubkey::new_from_array(array_at(data, 80)?);
        let template_id: [u8; 32] = array_at(data, 112)?;
        let config_hash: [u8; 32] = array_at(data, 144)?;
        let flags = u64::from_le_bytes(array_at(data, 176)?);
        let config_seq = u64::from_le_bytes(array_at(data, 184)?);
        if platform_config != Pubkey::default() {
            return Err(HookError::InvalidConfigData);
        }
        match authority_mode {
            AuthorityMode::Explicit => {
                if config_authority == Pubkey::default() {
                    return Err(HookError::InvalidConfigData);
                }
            }
            _ => {
                if config_authority != Pubkey::default() {
                    return Err(HookError::InvalidConfigData);
                }
            }
        }
        let params = &data[CONFIG_HEADER_LEN..];
        if compute_config_hash(&template_id, template_version, params) != config_hash {
            return Err(HookError::HashMismatch);
        }
        let mut buffer = [0u8; MAX_PARAMS_LEN];
        buffer[..params.len()].copy_from_slice(params);
        Ok(HookConfig {
            version,
            bump: data[9],
            list_bump: data[10],
            authority_mode,
            template_version,
            mint,
            platform_config,
            config_authority,
            template_id,
            config_hash,
            flags,
            config_seq,
            params_len,
            params: buffer,
        })
    }

    /// Serialize into `out`, which must be exactly `account_len()` bytes.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), HookError> {
        if out.len() != self.account_len() {
            return Err(HookError::InvalidConfigData);
        }
        out[..CONFIG_HEADER_LEN].fill(0);
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[8] = self.version;
        out[9] = self.bump;
        out[10] = self.list_bump;
        out[11] = self.authority_mode as u8;
        out[12..16].copy_from_slice(&self.template_version.to_le_bytes());
        out[16..48].copy_from_slice(self.mint.as_ref());
        out[48..80].copy_from_slice(self.platform_config.as_ref());
        out[80..112].copy_from_slice(self.config_authority.as_ref());
        out[112..144].copy_from_slice(&self.template_id);
        out[144..176].copy_from_slice(&self.config_hash);
        out[176..184].copy_from_slice(&self.flags.to_le_bytes());
        out[184..192].copy_from_slice(&self.config_seq.to_le_bytes());
        out[192..194].copy_from_slice(&self.params_len.to_le_bytes());
        out[CONFIG_HEADER_LEN..].copy_from_slice(self.params());
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.account_len()];
        // Length is exactly account_len(), so this cannot fail.
        let _ = self.encode_into(&mut out);
        out
    }

    /// The config must belong to `mint` and live at the PDA derived from its stored bump.
    pub fn verify_address(
        &self,
        program_id: &Pubkey,
        mint: &Pubkey,
        config_key: &Pubkey,
    ) -> Result<(), HookError> {
        if self.mint != *mint {
            return Err(HookError::InvalidConfigPda);
        }
        let derived =
            Pubkey::create_program_address(&[CONFIG_SEED, mint.as_ref(), &[self.bump]], program_id)
                .map_err(|_| HookError::InvalidConfigPda)?;
        if derived != *config_key {
            return Err(HookError::InvalidConfigPda);
        }
        Ok(())
    }
}

fn array_at<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], HookError> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or(HookError::InvalidConfigData)
}

// ---------------------------------------------------------------------------------------------
// Addresses and instruction builders
// ---------------------------------------------------------------------------------------------

pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED, mint.as_ref()], program_id)
}

pub fn validation_list_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    spl_transfer_hook_interface::get_extra_account_metas_address_and_bump_seed(mint, program_id)
}

/// The single seeds-based meta stored in every validation list.
pub fn config_extra_account_meta() -> Result<ExtraAccountMeta, ProgramError> {
    ExtraAccountMeta::new_with_seeds(
        &[
            Seed::Literal {
                bytes: CONFIG_SEED.to_vec(),
            },
            Seed::AccountKey { index: 1 },
        ],
        false,
        false,
    )
}

pub fn execute_instruction_data(amount: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&EXECUTE_DISCRIMINATOR);
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

/// Arguments of `InitializeHook`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializeHookArgs {
    pub authority_mode: u8,
    pub template_id: [u8; 32],
    pub template_version: u32,
    pub flags: u64,
    /// Must be non-zero in mode 2 and zero otherwise.
    pub config_authority: Pubkey,
    pub params: Vec<u8>,
}

impl InitializeHookArgs {
    /// A `max-transfer-v1` hook with the given limit.
    pub fn max_transfer(mode: AuthorityMode, limit: u64, config_authority: Pubkey) -> Self {
        InitializeHookArgs {
            authority_mode: mode as u8,
            template_id: TEMPLATE_MAX_TRANSFER_V1,
            template_version: MAX_TRANSFER_TEMPLATE_VERSION,
            flags: 0,
            config_authority,
            params: max_transfer_params(limit).to_vec(),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(INITIALIZE_HOOK_FIXED_LEN + self.params.len());
        data.extend_from_slice(&INITIALIZE_HOOK_DISCRIMINATOR);
        data.push(self.authority_mode);
        data.extend_from_slice(&self.template_version.to_le_bytes());
        data.extend_from_slice(&self.flags.to_le_bytes());
        data.extend_from_slice(&self.template_id);
        data.extend_from_slice(self.config_authority.as_ref());
        data.extend_from_slice(&(self.params.len() as u16).to_le_bytes());
        data.extend_from_slice(&self.params);
        data
    }

    fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() < INITIALIZE_HOOK_FIXED_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }
        let params_len = usize::from(u16::from_le_bytes([data[85], data[86]]));
        if params_len > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge.into());
        }
        if data.len() != INITIALIZE_HOOK_FIXED_LEN + params_len {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(InitializeHookArgs {
            authority_mode: data[8],
            template_version: u32::from_le_bytes(ix_array(data, 9)?),
            flags: u64::from_le_bytes(ix_array(data, 13)?),
            template_id: ix_array(data, 21)?,
            config_authority: Pubkey::new_from_array(ix_array(data, 53)?),
            params: data[INITIALIZE_HOOK_FIXED_LEN..].to_vec(),
        })
    }
}

fn ix_array<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or(ProgramError::InvalidInstructionData)
}

/// Atomically create the config and validation list of `mint`.
pub fn initialize_hook_instruction(
    program_id: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    payer: Pubkey,
    args: &InitializeHookArgs,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config_address(&mint, &program_id).0, false),
            AccountMeta::new(validation_list_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_program::system_program::id(), false),
        ],
        data: args.pack(),
    }
}

pub fn update_config_instruction(
    program_id: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    expected_seq: u64,
    template_version: u32,
    flags: u64,
    params: &[u8],
) -> Instruction {
    let mut data = Vec::with_capacity(UPDATE_CONFIG_FIXED_LEN + params.len());
    data.extend_from_slice(&UPDATE_CONFIG_DISCRIMINATOR);
    data.extend_from_slice(&expected_seq.to_le_bytes());
    data.extend_from_slice(&template_version.to_le_bytes());
    data.extend_from_slice(&flags.to_le_bytes());
    data.extend_from_slice(&(params.len() as u16).to_le_bytes());
    data.extend_from_slice(params);
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data,
    }
}

pub fn set_config_authority_instruction(
    program_id: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    new_authority: Pubkey,
) -> Instruction {
    let mut data = Vec::with_capacity(40);
    data.extend_from_slice(&SET_CONFIG_AUTHORITY_DISCRIMINATOR);
    data.extend_from_slice(new_authority.as_ref());
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data,
    }
}

// ---------------------------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------------------------

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let Some(discriminator) = instruction_data.get(..8) else {
        return Err(ProgramError::InvalidInstructionData);
    };
    if discriminator == EXECUTE_DISCRIMINATOR {
        process_execute(program_id, accounts, instruction_data)
    } else if discriminator == INITIALIZE_HOOK_DISCRIMINATOR {
        process_initialize_hook(program_id, accounts, instruction_data)
    } else if discriminator == UPDATE_CONFIG_DISCRIMINATOR {
        process_update_config(program_id, accounts, instruction_data)
    } else if discriminator == SET_CONFIG_AUTHORITY_DISCRIMINATOR {
        process_set_config_authority(program_id, accounts, instruction_data)
    } else if discriminator == SPL_INITIALIZE_LIST_DISCRIMINATOR
        || discriminator == SPL_UPDATE_LIST_DISCRIMINATOR
    {
        Err(HookError::SplInterfaceUnsupported.into())
    } else {
        Err(ProgramError::InvalidInstructionData)
    }
}

// ---------------------------------------------------------------------------------------------
// Shared validation helpers
// ---------------------------------------------------------------------------------------------

struct MintHookInfo {
    hook_program: Option<Pubkey>,
    extension_authority: Option<Pubkey>,
    mint_authority: Option<Pubkey>,
}

/// Mint must be Token-2022 owned and carry a TransferHook extension.
fn read_mint(mint: &AccountInfo) -> Result<MintHookInfo, ProgramError> {
    if mint.owner != &spl_token_2022::id() {
        return Err(HookError::MintOwnerNotToken2022.into());
    }
    let data = mint.try_borrow_data()?;
    let state = StateWithExtensions::<Mint>::unpack(&data)?;
    let extension = state
        .get_extension::<TransferHook>()
        .map_err(|_| HookError::MintHookExtensionMissing)?;
    Ok(MintHookInfo {
        hook_program: get_program_id(&state),
        extension_authority: Option::<Pubkey>::from(extension.authority),
        mint_authority: Option::<Pubkey>::from(state.base.mint_authority),
    })
}

fn require_hook_program(info: &MintHookInfo, program_id: &Pubkey) -> ProgramResult {
    if info.hook_program != Some(*program_id) {
        return Err(HookError::MintHookProgramMismatch.into());
    }
    Ok(())
}

fn require_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

fn require_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::Immutable);
    }
    Ok(())
}

fn require_system_program(account: &AccountInfo) -> ProgramResult {
    if account.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

/// A PDA that is about to be created must be unowned: system owned and empty.
fn require_uninitialized(
    account: &AccountInfo,
    program_id: &Pubkey,
    foreign_owner_error: HookError,
) -> ProgramResult {
    if account.owner == program_id {
        return Err(HookError::AlreadyInitialized.into());
    }
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(foreign_owner_error.into());
    }
    Ok(())
}

/// Create a program-owned PDA account. Survives a pre-funded PDA (anyone can send lamports to a
/// deterministic address): in that case top up to rent exemption, then allocate and assign.
fn create_pda_account<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    program_id: &Pubkey,
    space: usize,
    signer_seeds: &[&[u8]],
) -> ProgramResult {
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(HookError::AlreadyInitialized.into());
    }
    let required = Rent::get()?.minimum_balance(space);
    let space_u64 = u64::try_from(space).map_err(|_| ProgramError::InvalidArgument)?;
    let current = account.lamports();
    if current == 0 {
        invoke_signed(
            &system_instruction::create_account(
                payer.key,
                account.key,
                required,
                space_u64,
                program_id,
            ),
            &[payer.clone(), account.clone(), system_program.clone()],
            &[signer_seeds],
        )
    } else {
        if let Some(missing) = required.checked_sub(current).filter(|missing| *missing > 0) {
            invoke(
                &system_instruction::transfer(payer.key, account.key, missing),
                &[payer.clone(), account.clone(), system_program.clone()],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(account.key, space_u64),
            &[account.clone(), system_program.clone()],
            &[signer_seeds],
        )?;
        invoke_signed(
            &system_instruction::assign(account.key, program_id),
            &[account.clone(), system_program.clone()],
            &[signer_seeds],
        )
    }
}

/// Validate template, version, params and flags. Returns the hash-relevant params unchanged.
fn validate_template(
    template_id: &[u8; 32],
    template_version: u32,
    flags: u64,
    params: &[u8],
) -> Result<(), HookError> {
    if params.len() > MAX_PARAMS_LEN {
        return Err(HookError::ParamsTooLarge);
    }
    if *template_id != TEMPLATE_MAX_TRANSFER_V1 {
        return Err(HookError::UnknownTemplate);
    }
    if template_version != MAX_TRANSFER_TEMPLATE_VERSION {
        return Err(HookError::UnsupportedVersion);
    }
    if flags & !KNOWN_FLAGS_MASK != 0 {
        return Err(HookError::InvalidParams);
    }
    let bytes: [u8; MAX_TRANSFER_PARAMS_LEN] =
        params.try_into().map_err(|_| HookError::InvalidParams)?;
    if u64::from_le_bytes(bytes) == 0 {
        return Err(HookError::InvalidParams);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// InitializeHook
// ---------------------------------------------------------------------------------------------

fn process_initialize_hook(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = InitializeHookArgs::unpack(instruction_data)?;
    let [config, validation_list, mint, authority, payer, system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    require_system_program(system_program)?;
    require_signer(payer)?;
    require_writable(payer)?;
    require_signer(authority)?;
    require_writable(config)?;
    require_writable(validation_list)?;

    let mode = AuthorityMode::from_u8(args.authority_mode)?;
    let mint_info = read_mint(mint)?;
    require_hook_program(&mint_info, program_id)?;

    // The identity authority that may initialize the hook for this mint.
    let required_authority = match mode {
        AuthorityMode::MintAuthority => mint_info.mint_authority,
        AuthorityMode::ExtensionAuthority | AuthorityMode::Explicit | AuthorityMode::Immutable => {
            mint_info.extension_authority
        }
    }
    .ok_or(HookError::AuthorityUnavailable)?;
    if required_authority != *authority.key {
        return Err(HookError::AuthorityMismatch.into());
    }
    match mode {
        AuthorityMode::Explicit => {
            if args.config_authority == Pubkey::default() {
                return Err(HookError::AuthorityUnavailable.into());
            }
        }
        _ => {
            if args.config_authority != Pubkey::default() {
                return Err(HookError::InvalidParams.into());
            }
        }
    }
    validate_template(
        &args.template_id,
        args.template_version,
        args.flags,
        &args.params,
    )?;

    let (expected_config, bump) = config_address(mint.key, program_id);
    if config.key != &expected_config {
        return Err(HookError::InvalidConfigPda.into());
    }
    let (expected_list, list_bump) = validation_list_address(mint.key, program_id);
    if validation_list.key != &expected_list {
        return Err(HookError::InvalidValidationList.into());
    }
    require_uninitialized(config, program_id, HookError::InvalidConfigOwner)?;
    require_uninitialized(
        validation_list,
        program_id,
        HookError::InvalidValidationList,
    )?;

    let state = HookConfig::new(
        bump,
        list_bump,
        mode,
        args.template_id,
        args.template_version,
        *mint.key,
        args.config_authority,
        args.flags,
        &args.params,
    )?;
    let list_meta = config_extra_account_meta()?;
    let list_len = ExtraAccountMetaList::size_of(1)?;
    if list_len != VALIDATION_LIST_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let bump_seed = [bump];
    create_pda_account(
        payer,
        config,
        system_program,
        program_id,
        state.account_len(),
        &[CONFIG_SEED, mint.key.as_ref(), &bump_seed],
    )?;
    let list_bump_seed = [list_bump];
    create_pda_account(
        payer,
        validation_list,
        system_program,
        program_id,
        list_len,
        &[VALIDATION_LIST_SEED, mint.key.as_ref(), &list_bump_seed],
    )?;

    state.encode_into(&mut config.try_borrow_mut_data()?)?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut validation_list.try_borrow_mut_data()?,
        &[list_meta],
    )
}

// ---------------------------------------------------------------------------------------------
// UpdateConfig / SetConfigAuthority
// ---------------------------------------------------------------------------------------------

/// Ordered checks shared by every mutating instruction: config owner, discriminator and version
/// (decode), mint match, bump-derived address, mode, signer, and that the mint still points to
/// this program. Returns the decoded config.
fn authorize_mutation(
    program_id: &Pubkey,
    config: &AccountInfo,
    mint: &AccountInfo,
    authority: &AccountInfo,
    allow_modes: fn(AuthorityMode) -> Result<(), HookError>,
) -> Result<HookConfig, ProgramError> {
    require_writable(config)?;
    if config.owner != program_id {
        return Err(HookError::InvalidConfigOwner.into());
    }
    let state = HookConfig::decode(&config.try_borrow_data()?)?;
    state.verify_address(program_id, mint.key, config.key)?;
    if state.authority_mode == AuthorityMode::Immutable {
        return Err(HookError::ConfigImmutable.into());
    }
    allow_modes(state.authority_mode)?;
    require_signer(authority)?;
    let mint_info = read_mint(mint)?;
    let required = match state.authority_mode {
        AuthorityMode::ExtensionAuthority => mint_info.extension_authority,
        AuthorityMode::MintAuthority => mint_info.mint_authority,
        AuthorityMode::Explicit => Some(state.config_authority),
        AuthorityMode::Immutable => return Err(HookError::ConfigImmutable.into()),
    }
    .ok_or(HookError::AuthorityUnavailable)?;
    if required != *authority.key {
        return Err(HookError::AuthorityMismatch.into());
    }
    require_hook_program(&mint_info, program_id)?;
    Ok(state)
}

fn process_update_config(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() < UPDATE_CONFIG_FIXED_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let params_len = usize::from(u16::from_le_bytes([
        instruction_data[28],
        instruction_data[29],
    ]));
    if params_len > MAX_PARAMS_LEN {
        return Err(HookError::ParamsTooLarge.into());
    }
    if instruction_data.len() != UPDATE_CONFIG_FIXED_LEN + params_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    let expected_seq = u64::from_le_bytes(ix_array(instruction_data, 8)?);
    let template_version = u32::from_le_bytes(ix_array(instruction_data, 16)?);
    let flags = u64::from_le_bytes(ix_array(instruction_data, 20)?);
    let params = &instruction_data[UPDATE_CONFIG_FIXED_LEN..];

    let [config, mint, authority] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let current = authorize_mutation(program_id, config, mint, authority, |_| Ok(()))?;
    if current.config_seq != expected_seq {
        return Err(HookError::StaleConfigSeq.into());
    }
    validate_template(&current.template_id, template_version, flags, params)?;
    // Config size is fixed per template: `max-transfer-v1` params are always 8 bytes.
    if params.len() != current.params().len() {
        return Err(HookError::InvalidParams.into());
    }

    let mut next = HookConfig::new(
        current.bump,
        current.list_bump,
        current.authority_mode,
        current.template_id,
        template_version,
        current.mint,
        current.config_authority,
        flags,
        params,
    )?;
    next.config_seq = current
        .config_seq
        .checked_add(1)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    next.encode_into(&mut config.try_borrow_mut_data()?)?;
    Ok(())
}

fn process_set_config_authority(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() != 40 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let new_authority = Pubkey::new_from_array(ix_array(instruction_data, 8)?);
    let [config, mint, authority] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let current = authorize_mutation(program_id, config, mint, authority, |mode| match mode {
        AuthorityMode::Explicit => Ok(()),
        _ => Err(HookError::UnsupportedMode),
    })?;
    let mut next = current;
    if new_authority == Pubkey::default() {
        // One-way transition to Immutable.
        next.authority_mode = AuthorityMode::Immutable;
        next.config_authority = Pubkey::default();
    } else {
        next.config_authority = new_authority;
    }
    next.config_seq = current
        .config_seq
        .checked_add(1)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    next.encode_into(&mut config.try_borrow_mut_data()?)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Execute
// ---------------------------------------------------------------------------------------------

/// The token account must belong to `mint` and be mid-transfer (flag set by Token-2022 only).
fn require_transferring(account: &AccountInfo, mint: &Pubkey) -> ProgramResult {
    if account.owner != &spl_token_2022::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    if state.base.mint != *mint {
        return Err(HookError::AccountOrderMismatch.into());
    }
    let transferring = state
        .get_extension::<TransferHookAccount>()
        .map(|extension| bool::from(extension.transferring))
        .unwrap_or(false);
    if !transferring {
        return Err(HookError::NotDirectInvocation.into());
    }
    Ok(())
}

/// Validate the list by hand so a corrupt list returns an error instead of panicking inside
/// `ExtraAccountMetaList::check_account_infos` (which unwraps and subtracts unchecked).
fn validate_list_layout(data: &[u8]) -> ProgramResult {
    if data.len() != VALIDATION_LIST_LEN
        || data[..8] != EXECUTE_DISCRIMINATOR
        || data[8..12] != ((4 + 35) as u32).to_le_bytes()
        || data[12..16] != 1u32.to_le_bytes()
    {
        return Err(HookError::InvalidValidationList.into());
    }
    Ok(())
}

fn process_execute(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() != 16 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(ix_array(instruction_data, 8)?);
    if accounts.len() != EXECUTE_ACCOUNT_COUNT {
        return Err(HookError::WrongAccountCount.into());
    }
    let [source, mint, destination, _owner, validation_list, config] = accounts else {
        return Err(HookError::WrongAccountCount.into());
    };
    // Token-2022 builds the Execute CPI with every account read-only, so none of them may be
    // writable. The owner/authority slot is exempt: on a top-level (forged) call its writability
    // is the transaction-level flag, which is true for the fee payer, and such calls are rejected
    // by the transferring-flag guard below anyway.
    if [source, mint, destination, validation_list, config]
        .iter()
        .any(|account| account.is_writable)
    {
        return Err(HookError::AccountOrderMismatch.into());
    }

    let mint_info = read_mint(mint)?;
    require_hook_program(&mint_info, program_id)?;
    // Both sides must carry the transferring flag, which only Token-2022 sets during a transfer.
    require_transferring(source, mint.key)?;
    require_transferring(destination, mint.key)?;

    if config.owner != program_id {
        return Err(HookError::InvalidConfigOwner.into());
    }
    let state = HookConfig::decode(&config.try_borrow_data()?)?;
    state.verify_address(program_id, mint.key, config.key)?;

    let expected_list = Pubkey::create_program_address(
        &[VALIDATION_LIST_SEED, mint.key.as_ref(), &[state.list_bump]],
        program_id,
    )
    .map_err(|_| HookError::InvalidValidationList)?;
    if validation_list.key != &expected_list || validation_list.owner != program_id {
        return Err(HookError::InvalidValidationList.into());
    }
    {
        let list_data = validation_list.try_borrow_data()?;
        validate_list_layout(&list_data)?;
        ExtraAccountMetaList::check_account_infos::<ExecuteInstruction>(
            accounts,
            instruction_data,
            program_id,
            &list_data,
        )
        .map_err(|_| HookError::AccountOrderMismatch)?;
    }

    match state.version {
        1 => enforce_v1(&state, amount),
        _ => Err(HookError::UnsupportedVersion.into()),
    }
}

fn enforce_v1(state: &HookConfig, amount: u64) -> ProgramResult {
    if state.template_id != TEMPLATE_MAX_TRANSFER_V1 {
        return Err(HookError::UnknownTemplate.into());
    }
    if state.template_version != MAX_TRANSFER_TEMPLATE_VERSION {
        return Err(HookError::UnsupportedVersion.into());
    }
    // Inclusive: amount == limit passes.
    if amount > state.max_transfer_limit()? {
        return Err(HookError::TransferExceedsLimit.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spl_tlv_account_resolution::state::ExtraAccountMetaList as List;
    use spl_transfer_hook_interface::instruction::TransferHookInstruction;

    #[test]
    fn execute_instruction_data_matches_spl_interface() {
        let data = execute_instruction_data(0x0102_0304_0506_0708);
        assert_eq!(data[..8], EXECUTE_DISCRIMINATOR);
        assert_eq!(&data[8..], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(
            TransferHookInstruction::Execute { amount: 7 }.pack()[..8],
            EXECUTE_DISCRIMINATOR
        );
    }

    #[test]
    fn spl_list_discriminators_match_interface() {
        let init = TransferHookInstruction::InitializeExtraAccountMetaList {
            extra_account_metas: vec![],
        }
        .pack();
        assert_eq!(init[..8], SPL_INITIALIZE_LIST_DISCRIMINATOR);
        let update = TransferHookInstruction::UpdateExtraAccountMetaList {
            extra_account_metas: vec![],
        }
        .pack();
        assert_eq!(update[..8], SPL_UPDATE_LIST_DISCRIMINATOR);
    }

    #[test]
    fn error_codes_are_contiguous_from_0x7001_and_round_trip() {
        for (index, error) in HookError::ALL.iter().enumerate() {
            assert_eq!(error.code(), 0x7001 + index as u32);
            assert_eq!(HookError::from_code(error.code()), Some(*error));
            assert_eq!(
                ProgramError::from(*error),
                ProgramError::Custom(error.code())
            );
        }
        assert_eq!(HookError::TransferExceedsLimit.code(), 0x700b);
        assert_eq!(HookError::from_code(1), None);
    }

    #[test]
    fn canonical_list_has_the_hand_validated_layout() {
        assert_eq!(List::size_of(1).unwrap(), VALIDATION_LIST_LEN);
        let mut data = vec![0u8; VALIDATION_LIST_LEN];
        List::init::<ExecuteInstruction>(&mut data, &[config_extra_account_meta().unwrap()])
            .unwrap();
        validate_list_layout(&data).unwrap();
        // Corrupt variants are rejected without panicking.
        for mutate in [
            |d: &mut Vec<u8>| d.truncate(10),
            |d: &mut Vec<u8>| d[0] ^= 1,
            |d: &mut Vec<u8>| d[8] ^= 1,
            |d: &mut Vec<u8>| d[12] = 2,
            |d: &mut Vec<u8>| d.push(0),
        ] {
            let mut corrupt = data.clone();
            mutate(&mut corrupt);
            assert!(validate_list_layout(&corrupt).is_err());
        }
    }

    fn sample_config() -> HookConfig {
        HookConfig::new(
            254,
            253,
            AuthorityMode::Explicit,
            TEMPLATE_MAX_TRANSFER_V1,
            1,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            0,
            &max_transfer_params(1_000),
        )
        .unwrap()
    }

    #[test]
    fn config_round_trips_and_is_264_bytes() {
        let config = sample_config();
        let bytes = config.encode();
        assert_eq!(bytes.len(), 264);
        assert_eq!(HookConfig::decode(&bytes).unwrap(), config);
        assert_eq!(config.max_transfer_limit().unwrap(), 1_000);
    }

    #[test]
    fn config_decode_is_strict() {
        let bytes = sample_config().encode();
        let err = |data: &[u8]| HookConfig::decode(data).unwrap_err();
        assert_eq!(err(&bytes[..100]), HookError::InvalidConfigData);
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert_eq!(err(&bad), HookError::InvalidConfigData);
        let mut bad = bytes.clone();
        bad[8] = 2;
        assert_eq!(err(&bad), HookError::UnsupportedVersion);
        let mut bad = bytes.clone();
        bad.push(0);
        assert_eq!(err(&bad), HookError::InvalidConfigData);
        let mut bad = bytes.clone();
        bad[200] = 1;
        assert_eq!(err(&bad), HookError::InvalidConfigData);
        let mut bad = bytes.clone();
        bad[11] = 4;
        assert_eq!(err(&bad), HookError::UnsupportedMode);
        let mut bad = bytes.clone();
        bad[48] = 1;
        assert_eq!(err(&bad), HookError::InvalidConfigData);
        let mut bad = bytes.clone();
        bad[CONFIG_HEADER_LEN] ^= 1;
        assert_eq!(err(&bad), HookError::HashMismatch);
        let mut bad = bytes.clone();
        bad[192] = 0xff;
        bad[193] = 0xff;
        assert_eq!(err(&bad), HookError::ParamsTooLarge);
        // Non-explicit modes must not carry a config authority.
        let mut bad = bytes.clone();
        bad[11] = 0;
        assert_eq!(err(&bad), HookError::InvalidConfigData);
    }

    #[test]
    fn config_address_uses_the_stored_bump() {
        let program_id = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let (address, bump) = config_address(&mint, &program_id);
        let mut config = sample_config();
        config.mint = mint;
        config.bump = bump;
        config.verify_address(&program_id, &mint, &address).unwrap();
        config.bump = bump.wrapping_sub(1);
        assert_eq!(
            config.verify_address(&program_id, &mint, &address),
            Err(HookError::InvalidConfigPda)
        );
        assert_eq!(
            config.verify_address(&program_id, &Pubkey::new_unique(), &address),
            Err(HookError::InvalidConfigPda)
        );
    }

    #[test]
    fn initialize_hook_args_round_trip() {
        let args =
            InitializeHookArgs::max_transfer(AuthorityMode::Explicit, 77, Pubkey::new_unique());
        assert_eq!(InitializeHookArgs::unpack(&args.pack()).unwrap(), args);
        assert!(InitializeHookArgs::unpack(&args.pack()[..20]).is_err());
    }
}
