//! The two per-mint accounts: the config (read on every transfer) and the slot counter (written on
//! every buy).
//!
//! Config layout (little-endian, 180 bytes): `b"ABCONFIG"`, `bump u8`, `mint [32]`,
//! `active_until i64`, `max_buys_per_slot u16`, `venue_count u8`, `venues [[32]; 4]` (unused
//! entries are zero).
//!
//! Counter layout (19 bytes): `b"ABCOUNTR"`, `bump u8`, `slot u64`, `buys u16`.

use solana_program::pubkey::Pubkey;

use crate::{
    error::AntiBundleError,
    rule::{Params, MAX_VENUES},
};

pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"ABCONFIG";
pub const CONFIG_LEN: usize = 8 + 1 + 32 + 8 + 2 + 1 + 32 * MAX_VENUES;
pub const COUNTER_DISCRIMINATOR: [u8; 8] = *b"ABCOUNTR";
pub const COUNTER_LEN: usize = 8 + 1 + 8 + 2;

/// The config PDA of `mint`: seeds `["config", mint]`.
pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"config", mint.as_ref()], program_id)
}

/// The slot-counter PDA of `mint`: seeds `["counter", mint]`.
pub fn counter_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"counter", mint.as_ref()], program_id)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub bump: u8,
    pub mint: Pubkey,
    pub params: Params,
    /// The recognised venue vaults (1 to [`MAX_VENUES`]).
    pub venues: Vec<Pubkey>,
}

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], AntiBundleError> {
    data.get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(AntiBundleError::InvalidConfig)
}

impl Config {
    pub fn decode(data: &[u8]) -> Result<Self, AntiBundleError> {
        if data.len() != CONFIG_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(AntiBundleError::InvalidConfig);
        }
        let venue_count = data[51] as usize;
        if venue_count == 0 || venue_count > MAX_VENUES {
            return Err(AntiBundleError::InvalidConfig);
        }
        let venues = (0..venue_count)
            .map(|i| read(data, 52 + 32 * i).map(Pubkey::new_from_array))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            bump: data[8],
            mint: Pubkey::new_from_array(read(data, 9)?),
            params: Params {
                active_until: i64::from_le_bytes(read(data, 41)?),
                max_buys_per_slot: u16::from_le_bytes(read(data, 49)?),
            },
            venues,
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), AntiBundleError> {
        if out.len() != CONFIG_LEN || self.venues.len() > MAX_VENUES {
            return Err(AntiBundleError::InvalidConfig);
        }
        out.fill(0);
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.mint.as_ref());
        out[41..49].copy_from_slice(&self.params.active_until.to_le_bytes());
        out[49..51].copy_from_slice(&self.params.max_buys_per_slot.to_le_bytes());
        out[51] = self.venues.len() as u8;
        for (i, venue) in self.venues.iter().enumerate() {
            out[52 + 32 * i..84 + 32 * i].copy_from_slice(venue.as_ref());
        }
        Ok(())
    }
}

/// How many buys landed in which slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Counter {
    pub bump: u8,
    pub slot: u64,
    pub buys: u16,
}

impl Counter {
    pub fn decode(data: &[u8]) -> Result<Self, AntiBundleError> {
        let bad = AntiBundleError::InvalidState;
        if data.len() != COUNTER_LEN || data[..8] != COUNTER_DISCRIMINATOR {
            return Err(bad);
        }
        Ok(Self {
            bump: data[8],
            slot: u64::from_le_bytes(data[9..17].try_into().map_err(|_| bad)?),
            buys: u16::from_le_bytes(data[17..19].try_into().map_err(|_| bad)?),
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), AntiBundleError> {
        if out.len() != COUNTER_LEN {
            return Err(AntiBundleError::InvalidState);
        }
        out[..8].copy_from_slice(&COUNTER_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..17].copy_from_slice(&self.slot.to_le_bytes());
        out[17..19].copy_from_slice(&self.buys.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_counter_round_trip_and_reject_bad_shapes() {
        let config = Config {
            bump: 254,
            mint: Pubkey::new_unique(),
            params: Params {
                active_until: -1,
                max_buys_per_slot: 3,
            },
            venues: vec![Pubkey::new_unique(), Pubkey::new_unique()],
        };
        let mut data = vec![0; CONFIG_LEN];
        config.encode_into(&mut data).unwrap();
        assert_eq!(Config::decode(&data), Ok(config));
        assert!(Config::decode(&data[..CONFIG_LEN - 1]).is_err());
        data[51] = 0;
        assert!(Config::decode(&data).is_err(), "a config needs a venue");

        let counter = Counter {
            bump: 7,
            slot: 123,
            buys: 4,
        };
        let mut data = vec![0; COUNTER_LEN];
        counter.encode_into(&mut data).unwrap();
        assert_eq!(Counter::decode(&data), Ok(counter));
        data[0] ^= 1;
        assert!(Counter::decode(&data).is_err());
    }
}
