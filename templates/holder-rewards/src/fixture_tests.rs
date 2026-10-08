//! The bytes, addresses, instructions and numbers the TypeScript client must reproduce, written to
//! `tests/fixtures/typescript/holder-rewards.json`. Regenerate after an intentional change with
//! `UPDATE_GOLDEN=1 cargo test -p holder-rewards-hook typescript_fixture`.

use solana_program::{instruction::Instruction, pubkey::Pubkey};

use crate::{
    error::HolderRewardsError as E,
    instruction::{claim, register},
    rule::{Holder, Stream},
    state::{
        global_address, record_address, reward_vault_address, Global, Record, GLOBAL_LEN,
        RECORD_LEN,
    },
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn describe(instruction: &Instruction) -> String {
    let accounts = instruction
        .accounts
        .iter()
        .map(|meta| {
            format!(
                "{{\"pubkey\": \"{}\", \"signer\": {}, \"writable\": {}}}",
                meta.pubkey, meta.is_signer, meta.is_writable
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"data_hex\": \"{}\", \"accounts\": [{accounts}]}}",
        hex(&instruction.data)
    )
}

#[test]
fn typescript_fixture() {
    let program = Pubkey::new_from_array([0xE2; 32]);
    let mint = Pubkey::new_from_array([0x11; 32]);
    let reward_mint = Pubkey::new_from_array([0x12; 32]);
    let pool_vault = Pubkey::new_from_array([0x13; 32]);
    let holder_account = Pubkey::new_from_array([0x14; 32]);
    let reward_account = Pubkey::new_from_array([0x15; 32]);
    let payer = Pubkey::new_from_array([0x16; 32]);
    let reward_token_program = Pubkey::new_from_array([0x17; 32]);

    // A funded stream with two registered holders; one of them is the record under test.
    let mut stream = Stream::default();
    stream.fund(1_000, 7_200_000, 3_600).unwrap(); // 2,000 per second until 4,600
    let mut other = Holder::register(&mut stream, 300).unwrap();
    stream.advance(1_100).unwrap();
    let mut holder = Holder::register(&mut stream, 100).unwrap();
    stream.advance(1_400).unwrap();
    other.on_balance_change(&mut stream, 300, 300).unwrap();
    // The record under test was settled once, by a transfer that touched it, and then left alone.
    holder.on_balance_change(&mut stream, 100, 100).unwrap();
    stream.advance(1_500).unwrap();

    let (global_pda, global_bump) = global_address(&mint, &program);
    let (vault_pda, _) = reward_vault_address(&mint, &program);
    let (record_pda, record_bump) = record_address(&holder_account, &program);
    let global = Global {
        bump: global_bump,
        mint,
        reward_mint,
        reward_vault: vault_pda,
        pool_vault,
        stream,
        one_time: true,
    };
    let mut global_bytes = vec![0; GLOBAL_LEN];
    global.encode_into(&mut global_bytes).unwrap();
    let record = Record {
        bump: record_bump,
        token_account: holder_account,
        holder,
    };
    let mut record_bytes = vec![0; RECORD_LEN];
    record.encode_into(&mut record_bytes).unwrap();

    // What the record could claim at various times and balances: the program's own `settle`, run on
    // copies after advancing a copy of the stream, so none of it is the client's own reasoning.
    let cases = [
        (1_500, 100),
        (1_500, 40),
        (1_501, 100),
        (2_000, 100),
        (2_000, 250),
        (4_599, 100),
        (4_600, 100),
        (9_999, 100),
        (900, 100),
    ];
    let claimable = cases
        .iter()
        .map(|(now, balance)| {
            let mut s = stream;
            s.advance(*now).unwrap();
            let mut h = holder;
            h.settle(&s, *balance).unwrap();
            format!(
                "    {{\"now\": {now}, \"balance\": {balance}, \"claimable\": {}}}",
                h.earned
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let errors = [
        ("InvalidInstruction", E::InvalidInstruction),
        ("InvalidGlobal", E::InvalidGlobal),
        ("InvalidRecord", E::InvalidRecord),
        ("ExcludedAccount", E::ExcludedAccount),
        ("NotRegistered", E::NotRegistered),
        ("ZeroAmount", E::ZeroAmount),
        ("InvalidDuration", E::InvalidDuration),
        ("MathOverflow", E::MathOverflow),
        ("RewardMintHasHook", E::RewardMintHasHook),
        ("RewardAccountMismatch", E::RewardAccountMismatch),
        ("WrongOwner", E::WrongOwner),
        ("NothingToClaim", E::NothingToClaim),
        ("PoolVaultMismatch", E::PoolVaultMismatch),
        ("AlreadyFunded", E::AlreadyFunded),
    ]
    .map(|(name, error)| format!("    {{\"name\": \"{name}\", \"code\": {}}}", error.code()))
    .join(",\n");

    let rendered = format!(
        "{{\n  \"program_id\": \"{program}\",\n  \"mint\": \"{mint}\",\n  \"reward_mint\": \"{reward_mint}\",\n  \"holder_account\": \"{holder_account}\",\n  \"global_address\": \"{global_pda}\",\n  \"reward_vault_address\": \"{vault_pda}\",\n  \"record_address\": \"{record_pda}\",\n  \"global_hex\": \"{}\",\n  \"record_hex\": \"{}\",\n  \"claimable\": [\n{claimable}\n  ],\n  \"register\": {{\"payer\": \"{payer}\", \"instruction\": {}}},\n  \"claim\": {{\"owner\": \"{payer}\", \"owner_reward_account\": \"{reward_account}\", \"reward_token_program\": \"{reward_token_program}\", \"instruction\": {}}},\n  \"errors\": [\n{errors}\n  ]\n}}\n",
        hex(&global_bytes),
        hex(&record_bytes),
        describe(&register(&program, &payer, &mint, &holder_account)),
        describe(&claim(
            &program,
            &payer,
            &mint,
            &holder_account,
            &reward_account,
            &reward_mint,
            &reward_token_program
        )),
    );
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("typescript")
        .join("holder-rewards.json");
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
