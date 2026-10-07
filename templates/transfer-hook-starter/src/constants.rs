//! Seeds, discriminators, sizes and the one built-in template id.

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
pub(crate) const EXECUTE_ACCOUNT_COUNT: usize = 6;
pub(crate) const INITIALIZE_HOOK_FIXED_LEN: usize = 8 + 1 + 4 + 8 + 32 + 32 + 2;
pub(crate) const UPDATE_CONFIG_FIXED_LEN: usize = 8 + 8 + 4 + 8 + 2;

pub(crate) const fn padded_template_id(name: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < name.len() {
        out[i] = name[i];
        i += 1;
    }
    out
}
