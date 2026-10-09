//! Seeds, discriminators and sizes.

pub const CONFIG_SEED: &[u8] = b"hook-config";
pub const VALIDATION_LIST_SEED: &[u8] = b"extra-account-metas";
pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"HKCONFIG";
pub const CONFIG_VERSION: u8 = 1;
/// Bytes before the params in a config account: discriminator, version, two bumps, params
/// length, mint.
pub const CONFIG_HEADER_LEN: usize = 8 + 1 + 1 + 1 + 2 + 32;
pub const MAX_PARAMS_LEN: usize = 256;

pub const INITIALIZE_HOOK_DISCRIMINATOR: [u8; 8] = *b"HKINIT01";
/// SPL Transfer Hook interface `Execute` discriminator.
pub const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];
/// SPL `InitializeExtraAccountMetaList` discriminator (rejected, see crate docs).
pub const SPL_INITIALIZE_LIST_DISCRIMINATOR: [u8; 8] = [43, 34, 13, 49, 167, 88, 235, 235];
/// SPL `UpdateExtraAccountMetaList` discriminator (rejected, see crate docs).
pub const SPL_UPDATE_LIST_DISCRIMINATOR: [u8; 8] = [157, 105, 42, 146, 102, 85, 241, 174];

/// The default rule's params: one little-endian `u64` limit.
pub const MAX_TRANSFER_PARAMS_LEN: usize = 8;

/// Exact length of the validation list: TLV header (8 + 4) + pod slice prefix (4) + one meta (35).
pub const VALIDATION_LIST_LEN: usize = 8 + 4 + 4 + 35;
/// The validation list of every mint, byte for byte: TLV type (`Execute`), TLV length `4 + 35`,
/// one entry, then one seeds-based meta (discriminator `1`, packed seeds `Literal "hook-config"`
/// and `AccountKey { index: 1 }`, zero padding to 32 bytes, not signer, not writable). A unit test
/// pins it to what the SPL crate produces for `config_extra_account_meta`.
pub const CANONICAL_VALIDATION_LIST: [u8; VALIDATION_LIST_LEN] = [
    105, 37, 101, 197, 75, 251, 102, 26, // TLV type: Execute
    39, 0, 0, 0, // TLV length: 4 + 35
    1, 0, 0, 0, // one entry
    1, // meta discriminator: seeds
    1, 11, b'h', b'o', b'o', b'k', b'-', b'c', b'o', b'n', b'f', b'i', b'g', // Literal seed
    3, 1, // AccountKey { index: 1 }
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // padding to 32 bytes
    0, 0, // is_signer, is_writable
];
/// `Execute` instruction data: discriminator and a little-endian `u64` amount.
pub(crate) const EXECUTE_DATA_LEN: usize = 8 + 8;
pub(crate) const INITIALIZE_HOOK_FIXED_LEN: usize = 8 + 2;
