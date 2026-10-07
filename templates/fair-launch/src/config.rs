//! The two per-mint accounts: the launch config (read on every transfer) and the slot counter
//! (written on every buy).
//!
//! Config layout (little-endian, 117 bytes): `b"FLCONFIG"`, `bump u8`, `mint [32]`,
//! `pool_vault [32]`, `window_start i64`, `window_end i64`, `max_buy u64`, `max_wallet u64`,
//! `max_buys_per_slot u32`, `max_priority_micro_lamports u64`.
//!
//! Counter layout (21 bytes): `b"FLCOUNTR"`, `bump u8`, `slot u64`, `buys u32`.

use solana_program::pubkey::Pubkey;

use crate::{error::FairLaunchError, rule::Params};

pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"FLCONFIG";
pub const CONFIG_LEN: usize = 8 + 1 + 32 + 32 + 8 + 8 + 8 + 8 + 4 + 8;
pub const COUNTER_DISCRIMINATOR: [u8; 8] = *b"FLCOUNTR";
pub const COUNTER_LEN: usize = 8 + 1 + 8 + 4;

/// The config PDA of `mint`: seeds `["config", mint]`.
pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"config", mint.as_ref()], program_id)
}

/// The slot-counter PDA of `mint`: seeds `["counter", mint]`.
pub fn counter_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"counter", mint.as_ref()], program_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub bump: u8,
    pub mint: Pubkey,
    /// The pool's vault of the hooked token: a transfer out of it is a buy.
    pub pool_vault: Pubkey,
    pub params: Params,
}

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], FairLaunchError> {
    data.get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(FairLaunchError::InvalidConfig)
}

impl Config {
    pub fn decode(data: &[u8]) -> Result<Self, FairLaunchError> {
        if data.len() != CONFIG_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(FairLaunchError::InvalidConfig);
        }
        Ok(Self {
            bump: data[8],
            mint: Pubkey::new_from_array(read(data, 9)?),
            pool_vault: Pubkey::new_from_array(read(data, 41)?),
            params: Params {
                window_start: i64::from_le_bytes(read(data, 73)?),
                window_end: i64::from_le_bytes(read(data, 81)?),
                max_buy: u64::from_le_bytes(read(data, 89)?),
                max_wallet: u64::from_le_bytes(read(data, 97)?),
                max_buys_per_slot: u32::from_le_bytes(read(data, 105)?),
                max_priority_micro_lamports: u64::from_le_bytes(read(data, 109)?),
            },
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), FairLaunchError> {
        if out.len() != CONFIG_LEN {
            return Err(FairLaunchError::InvalidConfig);
        }
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.mint.as_ref());
        out[41..73].copy_from_slice(self.pool_vault.as_ref());
        out[73..81].copy_from_slice(&self.params.window_start.to_le_bytes());
        out[81..89].copy_from_slice(&self.params.window_end.to_le_bytes());
        out[89..97].copy_from_slice(&self.params.max_buy.to_le_bytes());
        out[97..105].copy_from_slice(&self.params.max_wallet.to_le_bytes());
        out[105..109].copy_from_slice(&self.params.max_buys_per_slot.to_le_bytes());
        out[109..117].copy_from_slice(&self.params.max_priority_micro_lamports.to_le_bytes());
        Ok(())
    }
}

/// How many buys landed in which slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Counter {
    pub bump: u8,
    pub slot: u64,
    pub buys: u32,
}

impl Counter {
    pub fn decode(data: &[u8]) -> Result<Self, FairLaunchError> {
        if data.len() != COUNTER_LEN || data[..8] != COUNTER_DISCRIMINATOR {
            return Err(FairLaunchError::InvalidCounter);
        }
        Ok(Self {
            bump: data[8],
            slot: u64::from_le_bytes(
                data[9..17]
                    .try_into()
                    .map_err(|_| FairLaunchError::InvalidCounter)?,
            ),
            buys: u32::from_le_bytes(
                data[17..21]
                    .try_into()
                    .map_err(|_| FairLaunchError::InvalidCounter)?,
            ),
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), FairLaunchError> {
        if out.len() != COUNTER_LEN {
            return Err(FairLaunchError::InvalidCounter);
        }
        out[..8].copy_from_slice(&COUNTER_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..17].copy_from_slice(&self.slot.to_le_bytes());
        out[17..21].copy_from_slice(&self.buys.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_counter_round_trip_and_reject_bad_shapes() {
        let config = Config {
            bump: 255,
            mint: Pubkey::new_unique(),
            pool_vault: Pubkey::new_unique(),
            params: Params {
                window_start: -3,
                window_end: 9,
                max_buy: 1,
                max_wallet: 2,
                max_buys_per_slot: 3,
                max_priority_micro_lamports: 4,
            },
        };
        let mut data = vec![0; CONFIG_LEN];
        config.encode_into(&mut data).unwrap();
        assert_eq!(Config::decode(&data), Ok(config));
        assert!(Config::decode(&data[..CONFIG_LEN - 1]).is_err());

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
