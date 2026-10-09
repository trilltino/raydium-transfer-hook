//! The two per-mint accounts: the launch config (read on every transfer) and the slot counter
//! (written on every buy).
//!
//! Config layout (little-endian, 214 bytes): `b"FLCONFIG"`, `bump u8`, `mint [32]`,
//! `venue_count u8`, `venues [[32]; 4]` (unused entries are zero), `window_start i64`,
//! `window_end i64`, `max_buy u64`, `max_wallet u64`, `max_buys_per_slot u32`,
//! `max_priority_micro_lamports u64`.
//!
//! Counter layout (21 bytes): `b"FLCOUNTR"`, `bump u8`, `slot u64`, `buys u32`.

use hook_kit::{canonical_list, list_len, pubkey_meta, seeded_meta};
use solana_program::{pubkey::Pubkey, sysvar};

use crate::{error::FairLaunchError, rule::Params};

pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"FLCONFIG";
/// The most venues (pool vaults whose outgoing transfers count as buys) one launch can name.
pub const MAX_VENUES: usize = 4;
pub const CONFIG_LEN: usize = 8 + 1 + 32 + 1 + 32 * MAX_VENUES + 8 + 8 + 8 + 8 + 4 + 8;
pub const COUNTER_DISCRIMINATOR: [u8; 8] = *b"FLCOUNTR";
pub const COUNTER_LEN: usize = 8 + 1 + 8 + 4;
/// Seed of the config PDA: `["config", mint]`.
pub const CONFIG_SEED: &[u8] = b"config";
/// Seed of the slot-counter PDA: `["counter", mint]`.
pub const COUNTER_SEED: &[u8] = b"counter";

/// The validation list of a launch without the priority-fee check: the config (read-only) and the
/// slot counter (writable), both found from the mint (instruction account 1).
pub const VALIDATION_LIST: [u8; list_len(2)] = canonical_list([
    seeded_meta(CONFIG_SEED, 1, false),
    seeded_meta(COUNTER_SEED, 1, true),
]);
/// The validation list of a launch with the priority-fee check: the same two accounts, then the
/// instructions sysvar (to read the declared fee).
pub const VALIDATION_LIST_WITH_FEE_CHECK: [u8; list_len(3)] = canonical_list([
    seeded_meta(CONFIG_SEED, 1, false),
    seeded_meta(COUNTER_SEED, 1, true),
    pubkey_meta(&sysvar::instructions::ID.to_bytes(), false),
]);

// Byte offsets of the packed config fields.
const BUMP: usize = 8;
const MINT: usize = 9;
const VENUE_COUNT: usize = 41;
const VENUES: usize = 42;
const PARAMS: usize = VENUES + 32 * MAX_VENUES;
const WINDOW_START: usize = PARAMS;
const WINDOW_END: usize = PARAMS + 8;
const MAX_BUY: usize = PARAMS + 16;
const MAX_WALLET: usize = PARAMS + 24;
const MAX_BUYS_PER_SLOT: usize = PARAMS + 32;
const MAX_PRIORITY: usize = PARAMS + 36;
// Byte offsets of the packed counter fields.
const COUNTER_BUMP: usize = 8;
const COUNTER_SLOT: usize = 9;
const COUNTER_BUYS: usize = 17;

/// The config PDA of `mint`.
#[must_use]
pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED, mint.as_ref()], program_id)
}

/// The slot-counter PDA of `mint`.
#[must_use]
pub fn counter_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[COUNTER_SEED, mint.as_ref()], program_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub bump: u8,
    pub mint: Pubkey,
    venue_count: u8,
    venues: [Pubkey; MAX_VENUES],
    pub params: Params,
}

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], FairLaunchError> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or(FairLaunchError::InvalidConfig)
}

impl Config {
    /// A config for `venues`: between one and [`MAX_VENUES`] distinct pool vaults of the hooked token.
    ///
    /// # Errors
    /// `InvalidVenues` unless there are one to [`MAX_VENUES`] distinct venues.
    pub fn new(
        bump: u8,
        mint: Pubkey,
        venues: &[Pubkey],
        params: Params,
    ) -> Result<Self, FairLaunchError> {
        let distinct = venues
            .iter()
            .enumerate()
            .all(|(i, venue)| !venues[..i].contains(venue));
        if venues.is_empty() || venues.len() > MAX_VENUES || !distinct {
            return Err(FairLaunchError::InvalidVenues);
        }
        let venue_count = u8::try_from(venues.len()).map_err(|_| FairLaunchError::InvalidVenues)?;
        let mut stored = [Pubkey::default(); MAX_VENUES];
        stored[..venues.len()].copy_from_slice(venues);
        Ok(Self {
            bump,
            mint,
            venue_count,
            venues: stored,
            params,
        })
    }

    /// The pool vaults whose outgoing transfers are buys.
    #[must_use]
    pub fn venues(&self) -> &[Pubkey] {
        &self.venues[..usize::from(self.venue_count)]
    }

    /// Strict parse of a config account's data.
    ///
    /// # Errors
    /// `InvalidConfig` unless the data is exactly a config with one to [`MAX_VENUES`] venues.
    pub fn decode(data: &[u8]) -> Result<Self, FairLaunchError> {
        if data.len() != CONFIG_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(FairLaunchError::InvalidConfig);
        }
        let venue_count = data[VENUE_COUNT];
        if venue_count == 0 || usize::from(venue_count) > MAX_VENUES {
            return Err(FairLaunchError::InvalidConfig);
        }
        let mut venues = [Pubkey::default(); MAX_VENUES];
        for (i, venue) in venues.iter_mut().enumerate() {
            *venue = Pubkey::new_from_array(read(data, VENUES + 32 * i)?);
        }
        Ok(Self {
            bump: data[BUMP],
            mint: Pubkey::new_from_array(read(data, MINT)?),
            venue_count,
            venues,
            params: Params {
                window_start: i64::from_le_bytes(read(data, WINDOW_START)?),
                window_end: i64::from_le_bytes(read(data, WINDOW_END)?),
                max_buy: u64::from_le_bytes(read(data, MAX_BUY)?),
                max_wallet: u64::from_le_bytes(read(data, MAX_WALLET)?),
                max_buys_per_slot: u32::from_le_bytes(read(data, MAX_BUYS_PER_SLOT)?),
                max_priority_micro_lamports: u64::from_le_bytes(read(data, MAX_PRIORITY)?),
            },
        })
    }

    /// Serialize into `out`, which must be exactly [`CONFIG_LEN`] bytes.
    ///
    /// # Errors
    /// `InvalidConfig` if `out` has the wrong length.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), FairLaunchError> {
        if out.len() != CONFIG_LEN {
            return Err(FairLaunchError::InvalidConfig);
        }
        out.fill(0);
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[BUMP] = self.bump;
        out[MINT..VENUE_COUNT].copy_from_slice(self.mint.as_ref());
        out[VENUE_COUNT] = self.venue_count;
        for (i, venue) in self.venues.iter().enumerate() {
            let at = VENUES + 32 * i;
            out[at..at + 32].copy_from_slice(venue.as_ref());
        }
        out[WINDOW_START..WINDOW_END].copy_from_slice(&self.params.window_start.to_le_bytes());
        out[WINDOW_END..MAX_BUY].copy_from_slice(&self.params.window_end.to_le_bytes());
        out[MAX_BUY..MAX_WALLET].copy_from_slice(&self.params.max_buy.to_le_bytes());
        out[MAX_WALLET..MAX_BUYS_PER_SLOT].copy_from_slice(&self.params.max_wallet.to_le_bytes());
        out[MAX_BUYS_PER_SLOT..MAX_PRIORITY]
            .copy_from_slice(&self.params.max_buys_per_slot.to_le_bytes());
        out[MAX_PRIORITY..CONFIG_LEN]
            .copy_from_slice(&self.params.max_priority_micro_lamports.to_le_bytes());
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Counter {
    pub bump: u8,
    /// The slot of the last buy.
    pub slot: u64,
    /// Buys in that slot.
    pub buys: u32,
}

impl Counter {
    /// Strict parse of a counter account's data.
    ///
    /// # Errors
    /// `InvalidCounter` unless the data is exactly a counter.
    pub fn decode(data: &[u8]) -> Result<Self, FairLaunchError> {
        let bad = |_| FairLaunchError::InvalidCounter;
        if data.len() != COUNTER_LEN || data[..8] != COUNTER_DISCRIMINATOR {
            return Err(FairLaunchError::InvalidCounter);
        }
        Ok(Self {
            bump: data[COUNTER_BUMP],
            slot: u64::from_le_bytes(read(data, COUNTER_SLOT).map_err(bad)?),
            buys: u32::from_le_bytes(read(data, COUNTER_BUYS).map_err(bad)?),
        })
    }

    /// Serialize into `out`, which must be exactly [`COUNTER_LEN`] bytes.
    ///
    /// # Errors
    /// `InvalidCounter` if `out` has the wrong length.
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), FairLaunchError> {
        if out.len() != COUNTER_LEN {
            return Err(FairLaunchError::InvalidCounter);
        }
        out[..8].copy_from_slice(&COUNTER_DISCRIMINATOR);
        out[COUNTER_BUMP] = self.bump;
        out[COUNTER_SLOT..COUNTER_BUYS].copy_from_slice(&self.slot.to_le_bytes());
        out[COUNTER_BUYS..COUNTER_LEN].copy_from_slice(&self.buys.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAMS: Params = Params {
        window_start: 1,
        window_end: 9,
        max_buy: 5,
        max_wallet: 6,
        max_buys_per_slot: 7,
        max_priority_micro_lamports: 8,
    };

    #[test]
    fn a_config_round_trips_with_one_to_four_venues() {
        let mint = Pubkey::new_unique();
        for count in 1..=MAX_VENUES {
            let venues: Vec<Pubkey> = (0..count).map(|_| Pubkey::new_unique()).collect();
            let config = Config::new(254, mint, &venues, PARAMS).unwrap();
            let mut bytes = vec![0; CONFIG_LEN];
            config.encode_into(&mut bytes).unwrap();
            let decoded = Config::decode(&bytes).unwrap();
            assert_eq!(decoded, config);
            assert_eq!(decoded.venues(), venues.as_slice());
        }
    }

    #[test]
    fn venues_must_be_one_to_four_and_distinct() {
        let mint = Pubkey::new_unique();
        let a = Pubkey::new_unique();
        assert_eq!(
            Config::new(1, mint, &[], PARAMS),
            Err(FairLaunchError::InvalidVenues)
        );
        assert_eq!(
            Config::new(1, mint, &[a, a], PARAMS),
            Err(FairLaunchError::InvalidVenues)
        );
        let five: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
        assert_eq!(
            Config::new(1, mint, &five, PARAMS),
            Err(FairLaunchError::InvalidVenues)
        );
    }

    #[test]
    fn the_counter_round_trips_and_rejects_bad_shapes() {
        let counter = Counter {
            bump: 7,
            slot: 123,
            buys: 4,
        };
        let mut data = vec![0; COUNTER_LEN];
        counter.encode_into(&mut data).unwrap();
        assert_eq!(Counter::decode(&data), Ok(counter));
        assert!(Counter::decode(&data[..COUNTER_LEN - 1]).is_err());
        assert!(counter.encode_into(&mut data[..COUNTER_LEN - 1]).is_err());
        data[0] ^= 1;
        assert!(Counter::decode(&data).is_err());
    }

    #[test]
    fn a_malformed_config_is_refused() {
        let config = Config::new(1, Pubkey::new_unique(), &[Pubkey::new_unique()], PARAMS).unwrap();
        let mut bytes = vec![0; CONFIG_LEN];
        config.encode_into(&mut bytes).unwrap();
        assert!(Config::decode(&bytes[..CONFIG_LEN - 1]).is_err());
        assert!(config.encode_into(&mut bytes[..CONFIG_LEN - 1]).is_err());
        let mut no_venues = bytes.clone();
        no_venues[41] = 0;
        assert!(Config::decode(&no_venues).is_err());
        let mut too_many = bytes.clone();
        too_many[41] = u8::try_from(MAX_VENUES + 1).unwrap();
        assert!(Config::decode(&too_many).is_err());
        let mut wrong_tag = bytes;
        wrong_tag[0] ^= 1;
        assert!(Config::decode(&wrong_tag).is_err());
    }

    #[test]
    fn the_validation_lists_declare_the_config_the_counter_and_optionally_the_sysvar() {
        assert_eq!(VALIDATION_LIST.len(), 86);
        assert_eq!(VALIDATION_LIST_WITH_FEE_CHECK.len(), 121);
        // The two lists share the TLV type and the first two entries.
        assert_eq!(VALIDATION_LIST[..8], VALIDATION_LIST_WITH_FEE_CHECK[..8]);
        assert_eq!(
            VALIDATION_LIST[16..],
            VALIDATION_LIST_WITH_FEE_CHECK[16..86]
        );
        // is_writable is the last byte of an entry: the config is read-only, the counter writable.
        assert_eq!(VALIDATION_LIST[16 + 34], 0);
        assert_eq!(VALIDATION_LIST[16 + 35 + 34], 1);
        // The sysvar entry is a plain address and read-only.
        let third = &VALIDATION_LIST_WITH_FEE_CHECK[86..];
        assert_eq!(third[0], 0);
        assert_eq!(third[1..33], sysvar::instructions::ID.to_bytes());
        assert_eq!(third[33..], [0, 0]);
    }
}
