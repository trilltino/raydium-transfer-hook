//! The versioned per-mint `HookConfig` account: strict encode/decode of its byte layout.

use solana_program::{hash::hashv, pubkey::Pubkey};

use crate::{authority::AuthorityMode, constants::*, error::HookError};

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
