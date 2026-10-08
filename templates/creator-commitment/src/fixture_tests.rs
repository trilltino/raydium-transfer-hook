//! The bytes, addresses and numbers the TypeScript client must reproduce, written to
//! `tests/fixtures/typescript/creator-commitment.json`. Regenerate after an intentional change with
//! `UPDATE_GOLDEN=1 cargo test -p creator-commitment-hook typescript_fixture`.

use solana_program::pubkey::Pubkey;

use crate::{
    config::{config_address, Config, CONFIG_LEN},
    error::CommitmentError as E,
    rule::Schedule,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn typescript_fixture() {
    let program = Pubkey::new_from_array([0xE1; 32]);
    let mint = Pubkey::new_from_array([0x11; 32]);
    let creator = Pubkey::new_from_array([0x22; 32]);
    let schedule = Schedule {
        locked_total: 9_000,
        start: 1_700_000_000,
        cliff: 1_700_000_100,
        end: 1_700_000_300,
    };
    let (address, bump) = config_address(&mint, &program);
    let config = Config {
        bump,
        mint,
        creator_account: creator,
        schedule,
    };
    let mut bytes = vec![0; CONFIG_LEN];
    config.encode_into(&mut bytes).unwrap();

    // Times that cover before the start, the cliff edge, mid-vesting (including a rounding case) and the end.
    let times = [
        schedule.start - 10,
        schedule.start,
        schedule.cliff - 1,
        schedule.cliff,
        schedule.cliff + 1,
        schedule.start + 137,
        schedule.end - 1,
        schedule.end,
        schedule.end + 1_000,
    ];
    let samples = times
        .iter()
        .map(|now| {
            format!(
                "    {{\"now\": {now}, \"locked\": {}}}",
                schedule.locked_at(*now)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let errors = [
        ("InvalidSchedule", E::InvalidSchedule),
        ("ZeroLockedAmount", E::ZeroLockedAmount),
        ("CreatorAccountMismatch", E::CreatorAccountMismatch),
        ("InsufficientBalanceAtInit", E::InsufficientBalanceAtInit),
        ("VestingFloorBreached", E::VestingFloorBreached),
        ("InvalidConfig", E::InvalidConfig),
        ("InvalidInstruction", E::InvalidInstruction),
    ]
    .map(|(name, error)| format!("    {{\"name\": \"{name}\", \"code\": {}}}", error.code()))
    .join(",\n");
    let rendered = format!(
        "{{\n  \"program_id\": \"{program}\",\n  \"mint\": \"{mint}\",\n  \"creator_account\": \"{creator}\",\n  \"config_address\": \"{address}\",\n  \"config_hex\": \"{}\",\n  \"schedule\": {{\"locked_total\": {}, \"start\": {}, \"cliff\": {}, \"end\": {}}},\n  \"locked_at\": [\n{samples}\n  ],\n  \"errors\": [\n{errors}\n  ]\n}}\n",
        hex(&bytes),
        schedule.locked_total,
        schedule.start,
        schedule.cliff,
        schedule.end,
    );
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("typescript")
        .join("creator-commitment.json");
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
        "fixture differs; rerun with UPDATE_GOLDEN=1 if the change is intended"
    );
}
