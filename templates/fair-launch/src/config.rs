//! The two per-mint accounts: the launch config (read on every transfer) and the slot counter
//! (written on every buy).
//!
//! Config layout (little-endian, 214 bytes): `b"FLCONFIG"`, `bump u8`, `mint [32]`,
//! `venue_count u8`, `venues [[32]; 4]` (unused entries are zero), `window_start i64`,
//! `window_end i64`, `max_buy u64`, `max_wallet u64`, `max_buys_per_slot u32`,
//! `max_priority_micro_lamports u64`.
//!
//! Counter layout (21 bytes): `b"FLCOUNTR"`, `bump u8`, `slot u64`, `buys u32`.

use solana_program::pubkey::Pubkey;

use crate::{error::FairLaunchError, rule::Params};

pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"FLCONFIG";
/// The most venues (pool vaults whose outgoing transfers count as buys) one launch can name.
pub const MAX_VENUES: usize = 4;
pub const CONFIG_LEN: usize = 8 + 1 + 32 + 1 + 32 * MAX_VENUES + 8 + 8 + 8 + 8 + 4 + 8;
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
    venue_count: u8,
    venues: [Pubkey; MAX_VENUES],
    pub params: Params,
}

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], FairLaunchError> {
    data.get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(FairLaunchError::InvalidConfig)
}

impl Config {
    /// A config for `venues`: between one and [`MAX_VENUES`] distinct pool vaults of the hooked token.
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
        let mut stored = [Pubkey::default(); MAX_VENUES];
        stored[..venues.len()].copy_from_slice(venues);
        Ok(Self {
            bump,
            mint,
            venue_count: venues.len() as u8,
            venues: stored,
            params,
        })
    }

    /// The pool vaults whose outgoing transfers are buys.
    pub fn venues(&self) -> &[Pubkey] {
        &self.venues[..usize::from(self.venue_count)]
    }

    pub fn decode(data: &[u8]) -> Result<Self, FairLaunchError> {
        if data.len() != CONFIG_LEN || data[..8] != CONFIG_DISCRIMINATOR {
            return Err(FairLaunchError::InvalidConfig);
        }
        let venue_count = data[41];
        if venue_count == 0 || usize::from(venue_count) > MAX_VENUES {
            return Err(FairLaunchError::InvalidConfig);
        }
        let mut venues = [Pubkey::default(); MAX_VENUES];
        for (i, venue) in venues.iter_mut().enumerate() {
            *venue = Pubkey::new_from_array(read(data, 42 + 32 * i)?);
        }
        let at = 42 + 32 * MAX_VENUES;
        Ok(Self {
            bump: data[8],
            mint: Pubkey::new_from_array(read(data, 9)?),
            venue_count,
            venues,
            params: Params {
                window_start: i64::from_le_bytes(read(data, at)?),
                window_end: i64::from_le_bytes(read(data, at + 8)?),
                max_buy: u64::from_le_bytes(read(data, at + 16)?),
                max_wallet: u64::from_le_bytes(read(data, at + 24)?),
                max_buys_per_slot: u32::from_le_bytes(read(data, at + 32)?),
                max_priority_micro_lamports: u64::from_le_bytes(read(data, at + 36)?),
            },
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), FairLaunchError> {
        if out.len() != CONFIG_LEN {
            return Err(FairLaunchError::InvalidConfig);
        }
        out.fill(0);
        out[..8].copy_from_slice(&CONFIG_DISCRIMINATOR);
        out[8] = self.bump;
        out[9..41].copy_from_slice(self.mint.as_ref());
        out[41] = self.venue_count;
        for (i, venue) in self.venues.iter().enumerate() {
            out[42 + 32 * i..74 + 32 * i].copy_from_slice(venue.as_ref());
        }
        let at = 42 + 32 * MAX_VENUES;
        out[at..at + 8].copy_from_slice(&self.params.window_start.to_le_bytes());
        out[at + 8..at + 16].copy_from_slice(&self.params.window_end.to_le_bytes());
        out[at + 16..at + 24].copy_from_slice(&self.params.max_buy.to_le_bytes());
        out[at + 24..at + 32].copy_from_slice(&self.params.max_wallet.to_le_bytes());
        out[at + 32..at + 36].copy_from_slice(&self.params.max_buys_per_slot.to_le_bytes());
        out[at + 36..at + 44]
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
    pub fn decode(data: &[u8]) -> Result<Self, FairLaunchError> {
        if data.len() != COUNTER_LEN || data[..8] != COUNTER_DISCRIMINATOR {
            return Err(FairLaunchError::InvalidCounter);
        }
        Ok(Self {
            bump: data[8],
            slot: u64::from_le_bytes(read(data, 9).map_err(|_| FairLaunchError::InvalidCounter)?),
            buys: u32::from_le_bytes(read(data, 17).map_err(|_| FairLaunchError::InvalidCounter)?),
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
        data[0] ^= 1;
        assert!(Counter::decode(&data).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The bytes and addresses the TypeScript client must decode and derive identically. Committed
    /// under `tests/fixtures/typescript/`; regenerate with `UPDATE_GOLDEN=1 cargo test -p
    /// fair-launch-hook typescript_fixture` after an intentional layout change.
    #[test]
    fn typescript_fixture() {
        use crate::error::FairLaunchError as E;

        let program = Pubkey::new_from_array([0xF1; 32]);
        let mint = Pubkey::new_from_array([0x11; 32]);
        let venues = [
            Pubkey::new_from_array([0x21; 32]),
            Pubkey::new_from_array([0x22; 32]),
        ];
        let params = Params {
            window_start: 1_700_000_000,
            window_end: 1_700_003_600,
            max_buy: 10_000,
            max_wallet: 50_000,
            max_buys_per_slot: 3,
            max_priority_micro_lamports: 1_000,
        };
        let (config_address, config_bump) = config_address(&mint, &program);
        let (counter_address, counter_bump) = counter_address(&mint, &program);
        let config = Config::new(config_bump, mint, &venues, params).unwrap();
        let mut config_bytes = vec![0; CONFIG_LEN];
        config.encode_into(&mut config_bytes).unwrap();
        let counter = Counter {
            bump: counter_bump,
            slot: 424_242,
            buys: 2,
        };
        let mut counter_bytes = vec![0; COUNTER_LEN];
        counter.encode_into(&mut counter_bytes).unwrap();
        let errors = [
            ("InvalidParams", E::InvalidParams),
            ("PoolVaultMismatch", E::PoolVaultMismatch),
            ("PerBuyCapExceeded", E::PerBuyCapExceeded),
            ("MaxWalletExceeded", E::MaxWalletExceeded),
            ("TooManyBuysInSlot", E::TooManyBuysInSlot),
            ("PriorityFeeTooHigh", E::PriorityFeeTooHigh),
            ("InvalidConfig", E::InvalidConfig),
            ("InvalidInstruction", E::InvalidInstruction),
            ("InvalidCounter", E::InvalidCounter),
            ("InvalidSysvar", E::InvalidSysvar),
            ("InvalidVenues", E::InvalidVenues),
        ]
        .map(|(name, error)| format!("    {{\"name\": \"{name}\", \"code\": {}}}", error.code()))
        .join(",\n");
        let rendered = format!(
            "{{\n  \"program_id\": \"{program}\",\n  \"mint\": \"{mint}\",\n  \"venues\": [\"{}\", \"{}\"],\n  \"config_address\": \"{config_address}\",\n  \"counter_address\": \"{counter_address}\",\n  \"config_hex\": \"{}\",\n  \"counter_hex\": \"{}\",\n  \"params\": {{\"window_start\": {}, \"window_end\": {}, \"max_buy\": {}, \"max_wallet\": {}, \"max_buys_per_slot\": {}, \"max_priority_micro_lamports\": {}}},\n  \"counter\": {{\"slot\": {}, \"buys\": {}}},\n  \"errors\": [\n{errors}\n  ]\n}}\n",
            venues[0],
            venues[1],
            hex(&config_bytes),
            hex(&counter_bytes),
            params.window_start,
            params.window_end,
            params.max_buy,
            params.max_wallet,
            params.max_buys_per_slot,
            params.max_priority_micro_lamports,
            counter.slot,
            counter.buys,
        );
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("typescript")
            .join("fair-launch.json");
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &rendered).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("missing fixture {}: {error}", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            rendered, expected,
            "fixture differs; rerun with UPDATE_GOLDEN=1 if the layout change is intended"
        );
    }

    #[test]
    fn a_malformed_config_is_refused() {
        let config = Config::new(1, Pubkey::new_unique(), &[Pubkey::new_unique()], PARAMS).unwrap();
        let mut bytes = vec![0; CONFIG_LEN];
        config.encode_into(&mut bytes).unwrap();
        assert!(Config::decode(&bytes[..CONFIG_LEN - 1]).is_err());
        let mut no_venues = bytes.clone();
        no_venues[41] = 0;
        assert!(Config::decode(&no_venues).is_err());
        let mut too_many = bytes.clone();
        too_many[41] = (MAX_VENUES + 1) as u8;
        assert!(Config::decode(&too_many).is_err());
        let mut wrong_tag = bytes;
        wrong_tag[0] ^= 1;
        assert!(Config::decode(&wrong_tag).is_err());
    }
}
