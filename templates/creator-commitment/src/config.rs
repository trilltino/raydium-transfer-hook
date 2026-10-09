//! The per-mint config account: which token account is locked, and its schedule.
//!
//! Layout (little-endian, 105 bytes): `b"CRCONFIG"`, `bump u8`, `mint [32]`, `creator_account [32]`,
//! `locked_total u64`, `start i64`, `cliff i64`, `end i64`.

use hook_kit::{canonical_list, list_len, seeded_meta};
use solana_program::pubkey::Pubkey;

use crate::{error::CommitmentError, rule::Schedule};

pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"CRCONFIG";
pub const CONFIG_LEN: usize = 8 + 1 + 32 + 32 + 8 + 8 + 8 + 8;
/// Seed of the config PDA: `["config", mint]`.
pub const CONFIG_SEED: &[u8] = b"config";

/// The validation list of every mint: one extra account, the config, found from the mint
/// (instruction account 1) and read-only.
pub const VALIDATION_LIST: [u8; list_len(1)] = canonical_list([seeded_meta(CONFIG_SEED, 1, false)]);

// Byte offsets of the packed fields.
const BUMP: usize = 8;
const MINT: usize = 9;
const CREATOR_ACCOUNT: usize = 41;
const LOCKED_TOTAL: usize = 73;
const START: usize = 81;
const CLIFF: usize = 89;
const END: usize = 97;

/// The config PDA of `mint`.
#[must_use]
pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED, mint.as_ref()], program_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub bump: u8,
    pub mint: Pubkey,
    /// The token account whose balance is subject to the floor.
    pub creator_account: Pubkey,
    pub schedule: Schedule,
}

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], CommitmentError> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or(CommitmentError::InvalidConfig)
}

impl Config {
    /// Strict parse of a config account's data.
    ///
    /// # Errors
    /// `InvalidConfig` unless the data is exactly a config.
    pub fn decode(data: &[u8]) -> Result<Self, CommitmentError> {
        if data.len() != CONFIG_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(CommitmentError::InvalidConfig);
        }
        Ok(Self {
            bump: data[BUMP],
            mint: Pubkey::new_from_array(read(data, MINT)?),
            creator_account: Pubkey::new_from_array(read(data, CREATOR_ACCOUNT)?),
            schedule: Schedule {
                locked_total: u64::from_le_bytes(read(data, LOCKED_TOTAL)?),
                start: i64::from_le_bytes(read(data, START)?),
                cliff: i64::from_le_bytes(read(data, CLIFF)?),
                end: i64::from_le_bytes(read(data, END)?),
            },
        })
    }

    /// Serialize into `out`, which must be exactly [`CONFIG_LEN`] bytes.
    ///
    /// # Errors
    /// `InvalidConfig` if `out` has the wrong length.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), CommitmentError> {
        if out.len() != CONFIG_LEN {
            return Err(CommitmentError::InvalidConfig);
        }
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[BUMP] = self.bump;
        out[MINT..CREATOR_ACCOUNT].copy_from_slice(self.mint.as_ref());
        out[CREATOR_ACCOUNT..LOCKED_TOTAL].copy_from_slice(self.creator_account.as_ref());
        out[LOCKED_TOTAL..START].copy_from_slice(&self.schedule.locked_total.to_le_bytes());
        out[START..CLIFF].copy_from_slice(&self.schedule.start.to_le_bytes());
        out[CLIFF..END].copy_from_slice(&self.schedule.cliff.to_le_bytes());
        out[END..CONFIG_LEN].copy_from_slice(&self.schedule.end.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_rejects_bad_shapes() {
        let config = Config {
            bump: 254,
            mint: Pubkey::new_unique(),
            creator_account: Pubkey::new_unique(),
            schedule: Schedule {
                locked_total: 7,
                start: -5,
                cliff: 0,
                end: 9,
            },
        };
        let mut data = vec![0; CONFIG_LEN];
        config.encode_into(&mut data).unwrap();
        assert_eq!(Config::decode(&data), Ok(config));
        assert!(Config::decode(&data[..CONFIG_LEN - 1]).is_err());
        assert!(config.encode_into(&mut data[..CONFIG_LEN - 1]).is_err());
        data[0] ^= 1;
        assert!(Config::decode(&data).is_err());
    }

    #[test]
    fn the_validation_list_declares_exactly_the_config() {
        // Seeds ["config", accounts[1]], read-only: the first byte after the header is the
        // seeded-meta discriminator and the last two bytes are is_signer, is_writable.
        assert_eq!(VALIDATION_LIST.len(), 51);
        assert_eq!(VALIDATION_LIST[16], 1);
        assert_eq!(&VALIDATION_LIST[17..19], &[1, CONFIG_SEED.len() as u8]);
        assert_eq!(&VALIDATION_LIST[19..25], CONFIG_SEED);
        assert_eq!(&VALIDATION_LIST[25..27], &[3, 1]);
        assert_eq!(&VALIDATION_LIST[49..], &[0, 0]);
    }
}
