//! A hook the stack knows **only by program id and a JSON description of its setup**.
//!
//! To this test the hook is a compiled `arbitrary_test_hook.so` and the JSON below. There is no
//! Rust type for the hook, no provider written for it, no change to the SDK, the Raydium builders
//! or the flows: the generic provider turns the description into instructions, and the same flows
//! that run every other hook run this one through real CPMM and CLMM pools.
//!
//! This is the proof of "permissionless": nothing in the stack was told this hook exists.

use program_test_flows::{reference, run, run_with, setup, Setup};
use raydium_hook_driver::GenericExternalHook;

/// What a third party would hand an integrator: where the hook is and how to set it up. Account
/// order, seeds and the instruction layout come from the hook's own documentation.
fn description(setup: &Setup) -> String {
    let program = setup.env.arbitrary_hook_program().unwrap();
    format!(
        r#"{{
  "program_id": "{program}",
  "setup": [{{
    "accounts": [
      {{ "key": "{{payer}}", "signer": true, "writable": true }},
      {{ "key": "{{hooked_mint}}" }},
      {{ "key": "{{payer}}", "signer": true }},
      {{ "pda": {{ "program": "{{program}}", "seeds": ["utf8:arb-policy", "key:{{hooked_mint}}"] }}, "writable": true }},
      {{ "pda": {{ "program": "{{program}}", "seeds": ["utf8:arb-stats", "key:{{hooked_mint}}"] }}, "writable": true }},
      {{ "pda": {{ "program": "{{program}}", "seeds": ["utf8:extra-account-metas", "key:{{hooked_mint}}"] }}, "writable": true }},
      {{ "key": "{{system_program}}" }}
    ],
    "data_hex": "41524249 4e495431 02000000"
  }}],
  "allowed_writable": [
    {{ "pda": {{ "program": "{{program}}", "seeds": ["utf8:arb-stats", "key:{{hooked_mint}}"] }} }}
  ],
  "state_account": {{
    "pda": {{ "program": "{{program}}", "seeds": ["utf8:arb-stats", "key:{{hooked_mint}}"] }}
  }},
  "refusals": [
    {{ "direction": "hooked_in",  "plan": {{ "repeat": {{ "times": 3 }} }}, "code": 36865 }},
    {{ "direction": "hooked_out", "plan": {{ "repeat": {{ "times": 3 }} }}, "code": 36865 }}
  ]
}}"#
    )
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_runs_a_hook_known_only_by_its_description() {
    let s = setup();
    let hook = GenericExternalHook::from_json(&description(&s)).unwrap();
    run("cpmm", &hook, &s).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_runs_a_hook_known_only_by_its_description() {
    let s = setup();
    let hook = GenericExternalHook::from_json(&description(&s)).unwrap();
    run("clmm", &hook, &s).await;
}

/// The described hook on the second mint, next to a different hook on the first: two unrelated
/// programs on the two legs of one swap, one of which the stack was never told about.
#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_runs_a_described_hook_beside_another_hook() {
    let s = setup();
    let described = GenericExternalHook::from_json(&description(&s)).unwrap();
    run_with("cpmm", &reference(&s), Some(&described), 0, &s).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_runs_a_described_hook_beside_another_hook() {
    let s = setup();
    let described = GenericExternalHook::from_json(&description(&s)).unwrap();
    run_with("clmm", &reference(&s), Some(&described), 0, &s).await;
}
