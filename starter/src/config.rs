//! The per-mint `HookConfig` account: strict encode/decode of its byte layout.

use solana_program::pubkey::Pubkey;

use crate::{constants::*, error::HookError};

/// Little-endian parameters of the default rule.
pub fn max_transfer_params(limit: u64) -> [u8; MAX_TRANSFER_PARAMS_LEN] {
    limit.to_le_bytes()
}

/// Decoded, validated config account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookConfig {
    pub version: u8,
    pub bump: u8,
    pub list_bump: u8,
    pub mint: Pubkey,
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

    /// The limit of the default rule.
    pub fn max_transfer_limit(&self) -> Result<u64, HookError> {
        let bytes: [u8; MAX_TRANSFER_PARAMS_LEN] = self
            .params()
            .try_into()
            .map_err(|_| HookError::InvalidConfigData)?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Build a config for a new mint.
    pub fn new(bump: u8, list_bump: u8, mint: Pubkey, params: &[u8]) -> Result<Self, HookError> {
        if params.len() > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge);
        }
        let mut buffer = [0u8; MAX_PARAMS_LEN];
        buffer[..params.len()].copy_from_slice(params);
        Ok(HookConfig {
            version: CONFIG_VERSION,
            bump,
            list_bump,
            mint,
            params_len: u16::try_from(params.len()).map_err(|_| HookError::ParamsTooLarge)?,
            params: buffer,
        })
    }

    /// Strict parse of a config account's data. Never panics on arbitrary bytes.
    ///
    /// Checks, in order: header length, discriminator, version (fail closed), params length and
    /// total length.
    pub fn decode(data: &[u8]) -> Result<Self, HookError> {
        if data.len() < CONFIG_HEADER_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(HookError::InvalidConfigData);
        }
        let version = data[8];
        if version != CONFIG_VERSION {
            return Err(HookError::UnsupportedVersion);
        }
        let params_len = u16::from_le_bytes([data[11], data[12]]);
        if usize::from(params_len) > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge);
        }
        if data.len() != CONFIG_HEADER_LEN + usize::from(params_len) {
            return Err(HookError::InvalidConfigData);
        }
        let mint = Pubkey::try_from(&data[13..45]).map_err(|_| HookError::InvalidConfigData)?;
        let params = &data[CONFIG_HEADER_LEN..];
        let mut buffer = [0u8; MAX_PARAMS_LEN];
        buffer[..params.len()].copy_from_slice(params);
        Ok(HookConfig {
            version,
            bump: data[9],
            list_bump: data[10],
            mint,
            params_len,
            params: buffer,
        })
    }

    /// Serialize into `out`, which must be exactly `account_len()` bytes.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), HookError> {
        if out.len() != self.account_len() {
            return Err(HookError::InvalidConfigData);
        }
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[8] = self.version;
        out[9] = self.bump;
        out[10] = self.list_bump;
        out[11..13].copy_from_slice(&self.params_len.to_le_bytes());
        out[13..45].copy_from_slice(self.mint.as_ref());
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
