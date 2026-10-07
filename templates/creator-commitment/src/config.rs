//! The per-mint config account: which token account is locked, and its schedule.
//!
//! Layout (little-endian, 105 bytes): `b"CRCONFIG"`, `bump u8`, `mint [32]`, `creator_account [32]`,
//! `locked_total u64`, `start i64`, `cliff i64`, `end i64`.

use solana_program::pubkey::Pubkey;

use crate::{error::CommitmentError, rule::Schedule};

pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"CRCONFIG";
pub const CONFIG_LEN: usize = 8 + 1 + 32 + 32 + 8 + 8 + 8 + 8;

/// The config PDA of `mint`: seeds `["config", mint]`.
pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"config", mint.as_ref()], program_id)
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
    data.get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(CommitmentError::InvalidConfig)
}

impl Config {
    pub fn decode(data: &[u8]) -> Result<Self, CommitmentError> {
        if data.len() != CONFIG_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(CommitmentError::InvalidConfig);
        }
        Ok(Self {
            bump: data[8],
            mint: Pubkey::new_from_array(read(data, 9)?),
            creator_account: Pubkey::new_from_array(read(data, 41)?),
            schedule: Schedule {
                locked_total: u64::from_le_bytes(read(data, 73)?),
                start: i64::from_le_bytes(read(data, 81)?),
                cliff: i64::from_le_bytes(read(data, 89)?),
                end: i64::from_le_bytes(read(data, 97)?),
            },
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), CommitmentError> {
        if out.len() != CONFIG_LEN {
            return Err(CommitmentError::InvalidConfig);
        }
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.mint.as_ref());
        out[41..73].copy_from_slice(self.creator_account.as_ref());
        out[73..81].copy_from_slice(&self.schedule.locked_total.to_le_bytes());
        out[81..89].copy_from_slice(&self.schedule.start.to_le_bytes());
        out[89..97].copy_from_slice(&self.schedule.cliff.to_le_bytes());
        out[97..105].copy_from_slice(&self.schedule.end.to_le_bytes());
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
        data[0] ^= 1;
        assert!(Config::decode(&data).is_err());
    }
}
