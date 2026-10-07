//! The two kinds of account: the per-mint global (the stream and what it pays with) and one
//! record per registered token account.
//!
//! Global layout (little-endian, 185 bytes): `b"LRGLOBAL"`, `bump u8`, `mint [32]`,
//! `reward_mint [32]`, `reward_vault [32]`, `pool_vault [32]`, `rate u64`, `period_finish i64`,
//! `last_update i64`, `index u128`, `eligible_supply u64`.
//!
//! Record layout (73 bytes): `b"LRHOLDER"`, `bump u8`, `token_account [32]`, `checkpoint u64`,
//! `index_paid u128`, `earned u64`.

use solana_program::pubkey::Pubkey;

use crate::{
    error::LoyaltyError,
    rule::{Holder, Stream},
};

pub const GLOBAL_DISCRIMINATOR: [u8; 8] = *b"LRGLOBAL";
pub const GLOBAL_LEN: usize = 8 + 1 + 32 * 4 + 8 + 8 + 8 + 16 + 8;
pub const RECORD_DISCRIMINATOR: [u8; 8] = *b"LRHOLDER";
pub const RECORD_LEN: usize = 8 + 1 + 32 + 8 + 16 + 8;

/// The global PDA of `mint`: seeds `["rewards", mint]`. It also owns the reward vault.
pub fn global_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"rewards", mint.as_ref()], program_id)
}

/// The reward vault of `mint`: a token account of the reward mint at seeds `["reward-vault", mint]`.
pub fn reward_vault_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"reward-vault", mint.as_ref()], program_id)
}

/// The holder record of a token account: seeds `["holder", token_account]`.
pub fn record_address(token_account: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"holder", token_account.as_ref()], program_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Global {
    pub bump: u8,
    pub mint: Pubkey,
    pub reward_mint: Pubkey,
    pub reward_vault: Pubkey,
    /// The pool's vault of the hooked token: it can never register, so it never earns.
    pub pool_vault: Pubkey,
    pub stream: Stream,
}

fn read<const N: usize>(
    data: &[u8],
    offset: usize,
    error: LoyaltyError,
) -> Result<[u8; N], LoyaltyError> {
    data.get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(error)
}

impl Global {
    pub fn decode(data: &[u8]) -> Result<Self, LoyaltyError> {
        let bad = LoyaltyError::InvalidGlobal;
        if data.len() != GLOBAL_LEN || data[..8] != GLOBAL_DISCRIMINATOR {
            return Err(bad);
        }
        Ok(Self {
            bump: data[8],
            mint: Pubkey::new_from_array(read(data, 9, bad)?),
            reward_mint: Pubkey::new_from_array(read(data, 41, bad)?),
            reward_vault: Pubkey::new_from_array(read(data, 73, bad)?),
            pool_vault: Pubkey::new_from_array(read(data, 105, bad)?),
            stream: Stream {
                rate: u64::from_le_bytes(read(data, 137, bad)?),
                period_finish: i64::from_le_bytes(read(data, 145, bad)?),
                last_update: i64::from_le_bytes(read(data, 153, bad)?),
                index: u128::from_le_bytes(read(data, 161, bad)?),
                eligible_supply: u64::from_le_bytes(read(data, 177, bad)?),
            },
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), LoyaltyError> {
        if out.len() != GLOBAL_LEN {
            return Err(LoyaltyError::InvalidGlobal);
        }
        out[..8].copy_from_slice(&GLOBAL_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.mint.as_ref());
        out[41..73].copy_from_slice(self.reward_mint.as_ref());
        out[73..105].copy_from_slice(self.reward_vault.as_ref());
        out[105..137].copy_from_slice(self.pool_vault.as_ref());
        out[137..145].copy_from_slice(&self.stream.rate.to_le_bytes());
        out[145..153].copy_from_slice(&self.stream.period_finish.to_le_bytes());
        out[153..161].copy_from_slice(&self.stream.last_update.to_le_bytes());
        out[161..177].copy_from_slice(&self.stream.index.to_le_bytes());
        out[177..185].copy_from_slice(&self.stream.eligible_supply.to_le_bytes());
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    pub bump: u8,
    pub token_account: Pubkey,
    pub holder: Holder,
}

impl Record {
    pub fn decode(data: &[u8]) -> Result<Self, LoyaltyError> {
        let bad = LoyaltyError::InvalidRecord;
        if data.len() != RECORD_LEN || data[..8] != RECORD_DISCRIMINATOR {
            return Err(bad);
        }
        Ok(Self {
            bump: data[8],
            token_account: Pubkey::new_from_array(read(data, 9, bad)?),
            holder: Holder {
                checkpoint: u64::from_le_bytes(read(data, 41, bad)?),
                index_paid: u128::from_le_bytes(read(data, 49, bad)?),
                earned: u64::from_le_bytes(read(data, 65, bad)?),
            },
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), LoyaltyError> {
        if out.len() != RECORD_LEN {
            return Err(LoyaltyError::InvalidRecord);
        }
        out[..8].copy_from_slice(&RECORD_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.token_account.as_ref());
        out[41..49].copy_from_slice(&self.holder.checkpoint.to_le_bytes());
        out[49..65].copy_from_slice(&self.holder.index_paid.to_le_bytes());
        out[65..73].copy_from_slice(&self.holder.earned.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_and_record_round_trip_and_reject_bad_shapes() {
        let global = Global {
            bump: 250,
            mint: Pubkey::new_unique(),
            reward_mint: Pubkey::new_unique(),
            reward_vault: Pubkey::new_unique(),
            pool_vault: Pubkey::new_unique(),
            stream: Stream {
                rate: 1,
                period_finish: -2,
                last_update: 3,
                index: u128::MAX - 1,
                eligible_supply: 5,
            },
        };
        let mut data = vec![0; GLOBAL_LEN];
        global.encode_into(&mut data).unwrap();
        assert_eq!(Global::decode(&data), Ok(global));
        assert!(Global::decode(&data[..GLOBAL_LEN - 1]).is_err());

        let record = Record {
            bump: 9,
            token_account: Pubkey::new_unique(),
            holder: Holder {
                checkpoint: 11,
                index_paid: u128::MAX,
                earned: 13,
            },
        };
        let mut data = vec![0; RECORD_LEN];
        record.encode_into(&mut data).unwrap();
        assert_eq!(Record::decode(&data), Ok(record));
        data[0] ^= 1;
        assert!(Record::decode(&data).is_err());
    }
}
