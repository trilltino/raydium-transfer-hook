//! The two per-mint accounts: the read-only policy and the writable stats counter.

use solana_program::pubkey::Pubkey;

use crate::{constants::*, error::ArbError};

/// Decoded stats account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stats {
    pub bump: u8,
    pub slot: u64,
    pub count: u32,
    pub total: u64,
}

impl Stats {
    pub fn decode(data: &[u8]) -> Result<Self, ArbError> {
        if data.len() != STATS_LEN || data[..8] != STATS_DISCRIMINATOR {
            return Err(ArbError::InvalidStats);
        }
        Ok(Self {
            bump: data[8],
            slot: u64::from_le_bytes(data[9..17].try_into().map_err(|_| ArbError::InvalidStats)?),
            count: u32::from_le_bytes(
                data[17..21]
                    .try_into()
                    .map_err(|_| ArbError::InvalidStats)?,
            ),
            total: u64::from_le_bytes(
                data[21..29]
                    .try_into()
                    .map_err(|_| ArbError::InvalidStats)?,
            ),
        })
    }

    pub(crate) fn encode_into(&self, out: &mut [u8]) -> Result<(), ArbError> {
        if out.len() != STATS_LEN {
            return Err(ArbError::InvalidStats);
        }
        out[..8].copy_from_slice(&STATS_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..17].copy_from_slice(&self.slot.to_le_bytes());
        out[17..21].copy_from_slice(&self.count.to_le_bytes());
        out[21..29].copy_from_slice(&self.total.to_le_bytes());
        Ok(())
    }
}

/// Decoded policy account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub bump: u8,
    pub mint: Pubkey,
    pub max_per_slot: u32,
}

impl Policy {
    pub fn decode(data: &[u8]) -> Result<Self, ArbError> {
        if data.len() != POLICY_LEN || data[..8] != POLICY_DISCRIMINATOR {
            return Err(ArbError::InvalidPolicy);
        }
        Ok(Self {
            bump: data[8],
            mint: Pubkey::new_from_array(
                data[9..41]
                    .try_into()
                    .map_err(|_| ArbError::InvalidPolicy)?,
            ),
            max_per_slot: u32::from_le_bytes(
                data[41..45]
                    .try_into()
                    .map_err(|_| ArbError::InvalidPolicy)?,
            ),
        })
    }

    pub(crate) fn encode_into(&self, out: &mut [u8]) -> Result<(), ArbError> {
        if out.len() != POLICY_LEN {
            return Err(ArbError::InvalidPolicy);
        }
        out[..8].copy_from_slice(&POLICY_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.mint.as_ref());
        out[41..45].copy_from_slice(&self.max_per_slot.to_le_bytes());
        Ok(())
    }
}
