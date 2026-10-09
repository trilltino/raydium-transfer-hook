//! The per-mint `HookConfig` account: strict encode/decode of its byte layout.

use solana_program::pubkey::Pubkey;

use crate::{
    constants::{
        CONFIG_DISCRIMINATOR, CONFIG_HEADER_LEN, CONFIG_SEED, CONFIG_VERSION, MAX_PARAMS_LEN,
        MAX_TRANSFER_PARAMS_LEN,
    },
    error::HookError,
};

// Byte offsets of the packed header (see the layout table in the crate docs).
const VERSION_OFFSET: usize = 8;
const BUMP_OFFSET: usize = 9;
const LIST_BUMP_OFFSET: usize = 10;
const PARAMS_LEN_OFFSET: usize = 11;
const MINT_OFFSET: usize = 13;
const MINT_END: usize = MINT_OFFSET + 32;

/// Little-endian parameters of the default rule.
pub fn max_transfer_params(limit: u64) -> [u8; MAX_TRANSFER_PARAMS_LEN] {
    limit.to_le_bytes()
}

/// Decoded, validated config account. `params` borrows the account data (or the caller's buffer
/// for a config that is about to be written), so decoding copies nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookConfig<'a> {
    pub version: u8,
    pub bump: u8,
    pub list_bump: u8,
    pub mint: Pubkey,
    /// Invariant: at most `MAX_PARAMS_LEN` bytes, enforced by `new` and `decode`.
    params: &'a [u8],
}

impl<'a> HookConfig<'a> {
    #[must_use]
    pub fn params(&self) -> &'a [u8] {
        self.params
    }

    #[must_use]
    pub fn account_len(&self) -> usize {
        CONFIG_HEADER_LEN + self.params.len()
    }

    /// The limit of the default rule.
    ///
    /// # Errors
    /// `InvalidConfigData` if the params are not exactly one little-endian `u64`.
    pub fn max_transfer_limit(&self) -> Result<u64, HookError> {
        let bytes: [u8; MAX_TRANSFER_PARAMS_LEN] = self
            .params
            .try_into()
            .map_err(|_| HookError::InvalidConfigData)?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Build a config for a new mint.
    ///
    /// # Errors
    /// `ParamsTooLarge` if `params` is longer than `MAX_PARAMS_LEN`.
    pub fn new(bump: u8, list_bump: u8, mint: Pubkey, params: &'a [u8]) -> Result<Self, HookError> {
        if params.len() > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge);
        }
        Ok(HookConfig {
            version: CONFIG_VERSION,
            bump,
            list_bump,
            mint,
            params,
        })
    }

    /// Strict parse of a config account's data. Never panics on arbitrary bytes.
    ///
    /// Checks, in order: header length, discriminator, version (fail closed), params length and
    /// total length.
    ///
    /// # Errors
    /// `InvalidConfigData`, `UnsupportedVersion` or `ParamsTooLarge`, per the checks above.
    pub fn decode(data: &'a [u8]) -> Result<Self, HookError> {
        if data.len() < CONFIG_HEADER_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(HookError::InvalidConfigData);
        }
        let version = data[VERSION_OFFSET];
        if version != CONFIG_VERSION {
            return Err(HookError::UnsupportedVersion);
        }
        let params_len = u16::from_le_bytes([data[PARAMS_LEN_OFFSET], data[PARAMS_LEN_OFFSET + 1]]);
        if usize::from(params_len) > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge);
        }
        if data.len() != CONFIG_HEADER_LEN + usize::from(params_len) {
            return Err(HookError::InvalidConfigData);
        }
        let mint = Pubkey::try_from(&data[MINT_OFFSET..MINT_END])
            .map_err(|_| HookError::InvalidConfigData)?;
        Ok(HookConfig {
            version,
            bump: data[BUMP_OFFSET],
            list_bump: data[LIST_BUMP_OFFSET],
            mint,
            params: &data[CONFIG_HEADER_LEN..],
        })
    }

    /// Serialize into `out`, which must be exactly `account_len()` bytes.
    ///
    /// # Errors
    /// `InvalidConfigData` if `out` has the wrong length.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), HookError> {
        if out.len() != self.account_len() {
            return Err(HookError::InvalidConfigData);
        }
        let params_len = u16::try_from(self.params.len()).map_err(|_| HookError::ParamsTooLarge)?;
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[VERSION_OFFSET] = self.version;
        out[BUMP_OFFSET] = self.bump;
        out[LIST_BUMP_OFFSET] = self.list_bump;
        out[PARAMS_LEN_OFFSET..MINT_OFFSET].copy_from_slice(&params_len.to_le_bytes());
        out[MINT_OFFSET..MINT_END].copy_from_slice(self.mint.as_ref());
        out[CONFIG_HEADER_LEN..].copy_from_slice(self.params);
        Ok(())
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.account_len()];
        // Length is exactly account_len() and params are bounded, so this cannot fail.
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
