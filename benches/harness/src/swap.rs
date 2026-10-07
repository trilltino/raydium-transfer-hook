//! The Raydium swap scenario: the bench hook on one or both mints, with `N` extra accounts per
//! hooked leg, run through the real CPMM or CLMM flow in-process.

use program_test_flows::{clone, context_with, Setup};
use raydium_hook_driver::{
    env::Evidence, run_clmm, run_cpmm, FlowInputs, GenericExternalHook, LocalChain,
};
use serde_json::json;
use solana_program_test::ProgramTest;

use crate::{report::SwapRow, transfer::bench_program_id};

/// A JSON description of the bench hook for the generic provider: everything the stack needs to
/// know about it. (Nothing here names a Rust type of the hook.)
pub fn bench_description(extras: u8, writable_counter: bool) -> String {
    let counter = json!({
        "pda": { "program": "{program}", "seeds": ["utf8:bench-counter", "key:{hooked_mint}"] }
    });
    let allowed: Vec<_> = (writable_counter && extras > 0)
        .then(|| counter.clone())
        .into_iter()
        .collect();
    json!({
        "program_id": bench_program_id().to_string(),
        "setup": [{
            "accounts": [
                { "key": "{payer}", "signer": true, "writable": true },
                { "key": "{payer}", "signer": true },
                { "key": "{hooked_mint}" },
                { "pda": { "program": "{program}", "seeds": ["utf8:bench-counter", "key:{hooked_mint}"] }, "writable": true },
                { "pda": { "program": "{program}", "seeds": ["utf8:extra-account-metas", "key:{hooked_mint}"] }, "writable": true },
                { "key": "{system_program}" }
            ],
            "data_hex": format!("00{extras:02x}{:02x}", writable_counter as u8)
        }],
        "allowed_writable": allowed
    })
    .to_string()
}

/// First number before the words "compute units" in an evidence detail.
fn compute_units(evidence: &[Evidence], direction: &str) -> Option<u64> {
    let step = format!("hooked swap (hooked token {direction})");
    let detail = &evidence.iter().find(|e| e.step == step)?.detail;
    let end = detail.find(" compute units")?;
    detail[..end].rsplit(' ').next()?.parse().ok()
}

/// Run one flow and record what it showed. `hooked_legs` is 1 (the bench hook on `mint_0`) or 2
/// (the same program on both mints).
pub async fn measure(
    setup: &Setup,
    amm: &str,
    hooked_legs: u8,
    extras: u8,
    writable_counter: bool,
) -> SwapRow {
    let mut row = SwapRow {
        amm: amm.to_string(),
        hooked_legs,
        extras,
        writable_counter,
        ..Default::default()
    };
    let id = bench_program_id();
    let first = match GenericExternalHook::from_json(&bench_description(extras, writable_counter)) {
        Ok(hook) => hook,
        Err(e) => {
            row.error = Some(format!("the hook description is invalid: {e}"));
            return row;
        }
    };
    let second = (hooked_legs == 2).then(|| {
        GenericExternalHook::from_json(&bench_description(extras, writable_counter))
            .expect("the same description")
    });
    let mut context = context_with(setup, |test: &mut ProgramTest| {
        test.add_program("bench_hook", id, None);
    })
    .await;
    let mut chain = LocalChain::with_payer(&mut context, clone(&setup.deployer));
    let mut inputs = FlowInputs::new(&setup.env, &first, setup.fee_receiver.as_ref());
    if let Some(second) = &second {
        inputs = inputs.with_second_hook(second);
    }
    let outcome = match amm {
        "cpmm" => run_cpmm(&mut chain, &inputs).await,
        _ => run_clmm(&mut chain, &inputs).await,
    };
    row.largest_transaction_bytes = Some(chain.largest_transaction);
    match outcome {
        Ok(evidence) => {
            row.completed = true;
            row.compute_units_in = compute_units(&evidence, "in");
            row.compute_units_out = compute_units(&evidence, "out");
        }
        Err(e) => row.error = Some(e.to_string()),
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_is_a_valid_hook_for_the_generic_provider() {
        for (extras, writable) in [(0, false), (1, false), (8, false), (4, true)] {
            GenericExternalHook::from_json(&bench_description(extras, writable))
                .unwrap_or_else(|e| panic!("N={extras} writable={writable}: {e}"));
        }
    }

    #[test]
    fn compute_units_are_read_from_the_evidence() {
        let evidence = vec![Evidence {
            flow: "cpmm".into(),
            step: "hooked swap (hooked token in)".into(),
            signature: None,
            detail: "hook ran once; simulation ok: 18 log lines, 85918 compute units".into(),
        }];
        assert_eq!(compute_units(&evidence, "in"), Some(85_918));
        assert_eq!(compute_units(&evidence, "out"), None);
    }
}
