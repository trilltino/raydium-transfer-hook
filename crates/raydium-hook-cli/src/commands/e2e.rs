//! `e2e`: run the checked end-to-end flows against a cluster.

use raydium_hook_driver::{
    env::Evidence, run_clmm, run_cpmm, ArbitraryHook, Environment, FlowInputs, HookSetup,
    ReferenceHook, RpcChain,
};
use solana_sdk::signature::Signer;

use super::explorer;
use crate::args::{keypair, Flags, Res};

pub(crate) async fn e2e(flags: &Flags) -> Res<()> {
    let env_path = flags.need("env")?;
    let mut env = Environment::load(env_path).map_err(|e| e.to_string())?;
    let payer = keypair(flags.need("keypair")?)?;
    let fee_receiver = flags.get("fee-receiver-keypair").map(keypair).transpose()?;
    let amm = flags.get("amm").unwrap_or("all");
    let which = flags.get("hook").unwrap_or("all");
    let reference = ReferenceHook {
        program_id: env.reference_hook_program().map_err(|e| e.to_string())?,
        max_transfer: 500,
    };
    let arbitrary = ArbitraryHook {
        program_id: env.arbitrary_hook_program().map_err(|e| e.to_string())?,
        max_per_slot: 2,
    };
    let mut hooks: Vec<&dyn HookSetup> = Vec::new();
    if matches!(which, "reference" | "all") {
        hooks.push(&reference);
    }
    if matches!(which, "arbitrary" | "all") {
        hooks.push(&arbitrary);
    }
    let mut amms = Vec::new();
    if matches!(amm, "cpmm" | "all") {
        amms.push("cpmm");
    }
    if matches!(amm, "clmm" | "all") {
        amms.push("clmm");
    }
    if hooks.is_empty() || amms.is_empty() {
        return Err("nothing to run: check --amm and --hook".into());
    }

    println!(
        "RAYDIUM TRANSFER HOOK E2E  environment {} ({})  wallet {}\n",
        env.name,
        env.cluster,
        payer.pubkey()
    );
    let mut chain = RpcChain::new(env.rpc_url.clone(), payer);
    let mut table: Vec<(String, Result<Vec<Evidence>, String>)> = Vec::new();
    for amm in &amms {
        for hook in &hooks {
            let label = format!("{amm} + {}", hook.name());
            println!("== {label}");
            let inputs = FlowInputs {
                env: &env,
                hook: *hook,
                fee_receiver_keypair: fee_receiver.as_ref(),
            };
            let result = match *amm {
                "cpmm" => run_cpmm(&mut chain, &inputs).await,
                _ => run_clmm(&mut chain, &inputs).await,
            }
            .map_err(|e| e.to_string());
            table.push((label, result));
        }
    }

    println!("\nRESULT");
    let mut failed = false;
    for (label, result) in &table {
        match result {
            Ok(evidence) => {
                println!("  PASS  {label}");
                for e in evidence {
                    if let Some(sig) = &e.signature {
                        if e.step.starts_with("hooked swap") {
                            println!("          {}: {}", e.step, explorer(&env, sig));
                        }
                    }
                }
            }
            Err(message) => {
                failed = true;
                println!("  FAIL  {label}\n          {message}");
            }
        }
    }
    if flags.has("record") {
        for (_, result) in &table {
            if let Ok(evidence) = result {
                env.evidence.extend(evidence.iter().cloned());
            }
        }
        env.save(env_path).map_err(|e| e.to_string())?;
        println!("\nevidence appended to {env_path}");
    }
    if failed {
        Err("one or more flows failed".into())
    } else {
        Ok(())
    }
}
