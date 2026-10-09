//! The two kinds of account: the per-mint global (the stream and what it pays with) and one
//! record per registered token account.
//!
//! Global layout (little-endian, 186 bytes): `b"LRGLOBAL"`, `bump u8`, `mint [32]`,
//! `reward_mint [32]`, `reward_vault [32]`, `pool_vault [32]`, `rate u64`, `period_finish i64`,
//! `last_update i64`, `index u128`, `eligible_supply u64`, `one_time u8` (0 or 1).
//!
//! Record layout (73 bytes): `b"LRHOLDER"`, `bump u8`, `token_account [32]`, `checkpoint u64`,
//! `index_paid u128`, `earned u64`.

use hook_kit::{canonical_list, list_len, seeded_meta};
use solana_program::pubkey::Pubkey;

use crate::{
    error::HolderRewardsError,
    rule::{Holder, Stream},
};

pub const GLOBAL_DISCRIMINATOR: [u8; 8] = *b"LRGLOBAL";
pub const GLOBAL_LEN: usize = 8 + 1 + 32 * 4 + 8 + 8 + 8 + 16 + 8 + 1;
pub const RECORD_DISCRIMINATOR: [u8; 8] = *b"LRHOLDER";
pub const RECORD_LEN: usize = 8 + 1 + 32 + 8 + 16 + 8;
/// Seed of the global PDA: `["rewards", mint]`. It also owns the reward vault.
pub const REWARDS_SEED: &[u8] = b"rewards";
/// Seed of the reward vault: `["reward-vault", mint]`.
pub const REWARD_VAULT_SEED: &[u8] = b"reward-vault";
/// Seed of a holder record: `["holder", token_account]`.
pub const HOLDER_SEED: &[u8] = b"holder";

/// The validation list of every mint: three writable extra accounts. Instruction account 0 is the
/// source, 1 the mint, 2 the destination; the extras are the global (from the mint), the source's
/// record and the destination's record.
pub const VALIDATION_LIST: [u8; list_len(3)] = canonical_list([
    seeded_meta(REWARDS_SEED, 1, true),
    seeded_meta(HOLDER_SEED, 0, true),
    seeded_meta(HOLDER_SEED, 2, true),
]);

// Byte offsets of the packed global fields.
const G_BUMP: usize = 8;
const G_MINT: usize = 9;
const G_REWARD_MINT: usize = 41;
const G_REWARD_VAULT: usize = 73;
const G_POOL_VAULT: usize = 105;
const G_RATE: usize = 137;
const G_PERIOD_FINISH: usize = 145;
const G_LAST_UPDATE: usize = 153;
const G_INDEX: usize = 161;
const G_ELIGIBLE: usize = 177;
const G_ONE_TIME: usize = 185;
// Byte offsets of the packed record fields.
const R_BUMP: usize = 8;
const R_TOKEN_ACCOUNT: usize = 9;
const R_CHECKPOINT: usize = 41;
const R_INDEX_PAID: usize = 49;
const R_EARNED: usize = 65;

/// The global PDA of `mint`.
#[must_use]
pub fn global_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[REWARDS_SEED, mint.as_ref()], program_id)
}

/// The reward vault of `mint`: a token account of the reward mint.
#[must_use]
pub fn reward_vault_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[REWARD_VAULT_SEED, mint.as_ref()], program_id)
}

/// The holder record of a token account.
#[must_use]
pub fn record_address(token_account: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[HOLDER_SEED, token_account.as_ref()], program_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Global {
    pub bump: u8,
    pub mint: Pubkey,
    pub reward_mint: Pubkey,
    pub reward_vault: Pubkey,
    /// The pool vault of the hooked token: it can never register, so it never earns.
    pub pool_vault: Pubkey,
    pub stream: Stream,
    /// A one-time allocation (a spin-off): it can be funded once. Otherwise it can be topped up.
    pub one_time: bool,
}

fn read<const N: usize>(
    data: &[u8],
    offset: usize,
    error: HolderRewardsError,
) -> Result<[u8; N], HolderRewardsError> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or(error)
}

impl Global {
    /// Strict parse of a global account's data.
    ///
    /// # Errors
    /// `InvalidGlobal` unless the data is exactly a global.
    pub fn decode(data: &[u8]) -> Result<Self, HolderRewardsError> {
        let bad = HolderRewardsError::InvalidGlobal;
        if data.len() != GLOBAL_LEN || data[..8] != GLOBAL_DISCRIMINATOR {
            return Err(bad);
        }
        Ok(Self {
            bump: data[G_BUMP],
            mint: Pubkey::new_from_array(read(data, G_MINT, bad)?),
            reward_mint: Pubkey::new_from_array(read(data, G_REWARD_MINT, bad)?),
            reward_vault: Pubkey::new_from_array(read(data, G_REWARD_VAULT, bad)?),
            pool_vault: Pubkey::new_from_array(read(data, G_POOL_VAULT, bad)?),
            stream: Stream {
                rate: u64::from_le_bytes(read(data, G_RATE, bad)?),
                period_finish: i64::from_le_bytes(read(data, G_PERIOD_FINISH, bad)?),
                last_update: i64::from_le_bytes(read(data, G_LAST_UPDATE, bad)?),
                index: u128::from_le_bytes(read(data, G_INDEX, bad)?),
                eligible_supply: u64::from_le_bytes(read(data, G_ELIGIBLE, bad)?),
            },
            one_time: match data[G_ONE_TIME] {
                0 => false,
                1 => true,
                _ => return Err(bad),
            },
        })
    }

    /// Serialize into `out`, which must be exactly [`GLOBAL_LEN`] bytes.
    ///
    /// # Errors
    /// `InvalidGlobal` if `out` has the wrong length.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), HolderRewardsError> {
        if out.len() != GLOBAL_LEN {
            return Err(HolderRewardsError::InvalidGlobal);
        }
        out[..8].copy_from_slice(&GLOBAL_DISCRIMINATOR);
        out[G_BUMP] = self.bump;
        out[G_MINT..G_REWARD_MINT].copy_from_slice(self.mint.as_ref());
        out[G_REWARD_MINT..G_REWARD_VAULT].copy_from_slice(self.reward_mint.as_ref());
        out[G_REWARD_VAULT..G_POOL_VAULT].copy_from_slice(self.reward_vault.as_ref());
        out[G_POOL_VAULT..G_RATE].copy_from_slice(self.pool_vault.as_ref());
        out[G_RATE..G_PERIOD_FINISH].copy_from_slice(&self.stream.rate.to_le_bytes());
        out[G_PERIOD_FINISH..G_LAST_UPDATE]
            .copy_from_slice(&self.stream.period_finish.to_le_bytes());
        out[G_LAST_UPDATE..G_INDEX].copy_from_slice(&self.stream.last_update.to_le_bytes());
        out[G_INDEX..G_ELIGIBLE].copy_from_slice(&self.stream.index.to_le_bytes());
        out[G_ELIGIBLE..G_ONE_TIME].copy_from_slice(&self.stream.eligible_supply.to_le_bytes());
        out[G_ONE_TIME] = u8::from(self.one_time);
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
    /// Strict parse of a holder record's data.
    ///
    /// # Errors
    /// `InvalidRecord` unless the data is exactly a record.
    pub fn decode(data: &[u8]) -> Result<Self, HolderRewardsError> {
        let bad = HolderRewardsError::InvalidRecord;
        if data.len() != RECORD_LEN || data[..8] != RECORD_DISCRIMINATOR {
            return Err(bad);
        }
        Ok(Self {
            bump: data[R_BUMP],
            token_account: Pubkey::new_from_array(read(data, R_TOKEN_ACCOUNT, bad)?),
            holder: Holder {
                checkpoint: u64::from_le_bytes(read(data, R_CHECKPOINT, bad)?),
                index_paid: u128::from_le_bytes(read(data, R_INDEX_PAID, bad)?),
                earned: u64::from_le_bytes(read(data, R_EARNED, bad)?),
            },
        })
    }

    /// Serialize into `out`, which must be exactly [`RECORD_LEN`] bytes.
    ///
    /// # Errors
    /// `InvalidRecord` if `out` has the wrong length.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), HolderRewardsError> {
        if out.len() != RECORD_LEN {
            return Err(HolderRewardsError::InvalidRecord);
        }
        out[..8].copy_from_slice(&RECORD_DISCRIMINATOR);
        out[R_BUMP] = self.bump;
        out[R_TOKEN_ACCOUNT..R_CHECKPOINT].copy_from_slice(self.token_account.as_ref());
        out[R_CHECKPOINT..R_INDEX_PAID].copy_from_slice(&self.holder.checkpoint.to_le_bytes());
        out[R_INDEX_PAID..R_EARNED].copy_from_slice(&self.holder.index_paid.to_le_bytes());
        out[R_EARNED..RECORD_LEN].copy_from_slice(&self.holder.earned.to_le_bytes());
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
            one_time: true,
        };
        let mut data = vec![0; GLOBAL_LEN];
        global.encode_into(&mut data).unwrap();
        assert_eq!(Global::decode(&data), Ok(global));
        // The mode is one byte, 0 or 1.
        data[GLOBAL_LEN - 1] = 2;
        assert!(Global::decode(&data).is_err());
        data[GLOBAL_LEN - 1] = 0;
        assert!(!Global::decode(&data).unwrap().one_time);
        global.encode_into(&mut data).unwrap();
        assert!(Global::decode(&data[..GLOBAL_LEN - 1]).is_err());
        assert!(global.encode_into(&mut data[..GLOBAL_LEN - 1]).is_err());

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
        assert!(record.encode_into(&mut data[..RECORD_LEN - 1]).is_err());
        data[0] ^= 1;
        assert!(Record::decode(&data).is_err());
    }

    #[test]
    fn the_validation_list_declares_the_global_and_both_records_as_writable() {
        assert_eq!(VALIDATION_LIST.len(), 121);
        for entry in 0..3 {
            let at = 16 + 35 * entry;
            assert_eq!(VALIDATION_LIST[at], 1, "entry {entry} is a seeded PDA");
            assert_eq!(VALIDATION_LIST[at + 33], 0, "entry {entry} is not a signer");
            assert_eq!(VALIDATION_LIST[at + 34], 1, "entry {entry} is writable");
        }
        // The account index in each entry's seeds: the global comes from the mint (1), the
        // records from the source (0) and the destination (2).
        let index_of =
            |entry: usize, seed: &[u8]| VALIDATION_LIST[16 + 35 * entry + 4 + seed.len()];
        assert_eq!(index_of(0, REWARDS_SEED), 1);
        assert_eq!(index_of(1, HOLDER_SEED), 0);
        assert_eq!(index_of(2, HOLDER_SEED), 2);
    }
}
