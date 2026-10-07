//! `setup.json` describes this hook's setup to the generic provider that `raydium-hook e2e
//! --hook-dir` uses. It is data, so check it against the real encoding: if you change the rule or
//! `InitializeHook`, this test tells you to update the file.

use transfer_hook_starter::{AuthorityMode, HookError, InitializeHookArgs};

const SETUP_JSON: &str = include_str!("../setup.json");

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn setup_json_initialises_the_default_rule() {
    let args = InitializeHookArgs::max_transfer(
        AuthorityMode::ExtensionAuthority,
        500,
        Default::default(),
    );
    assert!(
        SETUP_JSON.contains(&format!("\"data_hex\": \"{}\"", hex(&args.pack()))),
        "setup.json's InitializeHook data no longer matches the program's encoding"
    );
}

#[test]
fn setup_json_expects_the_rules_refusal_code() {
    let code = HookError::TransferExceedsLimit as u32;
    assert!(
        SETUP_JSON.contains(&format!("\"code\": {code}")),
        "setup.json's refusal code no longer matches HookError::TransferExceedsLimit"
    );
    assert!(
        SETUP_JSON.contains("\"amount_in\": 600"),
        "a refusal must exceed the 500 limit"
    );
}
