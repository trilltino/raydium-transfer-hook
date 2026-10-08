//! `e2e`: run the checked end-to-end flows against a cluster.

use std::path::PathBuf;

use raydium_hook_driver::{
    chain::Chain, env::Evidence, inspect_readiness, run_clmm_session, run_cpmm_session, FlowInputs,
    GenericExternalHook, HookSetup, RpcChain, Session,
};
use solana_sdk::signature::Signer;

use super::{
    explorer, hook,
    table::{self, Subject},
};
use crate::args::{keypair, Flags, Res};

/// The hooks this repository ships, by CLI name and environment key. `all` runs every one the
/// environment has a program for.
const SHIPPED: &[(&str, Option<&str>)] = &[
    ("reference", None),
    ("arbitrary", None),
    ("creator-commitment", Some("creator_commitment")),
    ("fair-launch", Some("fair_launch")),
    ("fair-launch-per-slot", Some("fair_launch")),
    ("holder-rewards", Some("holder_rewards")),
    ("holder-rewards-one-time", Some("holder_rewards")),
];

pub(crate) async fn e2e(flags: &Flags) -> Res<()> {
    let env_path = flags.need("env")?;
    let mut env = raydium_hook_driver::Environment::load(env_path).map_err(|e| e.to_string())?;
    let payer = keypair(flags.need("keypair")?)?;
    let fee_receiver = flags.get("fee-receiver-keypair").map(keypair).transpose()?;
    let amm = flags.get("amm").unwrap_or("all");

    // The hooks to run: one the author built (`--hook-dir`), one described only as data
    // (`--setup`), or the shipped ones by name. Each with whether this repository ships it, decided
    // before anything is deployed (a deploy records the new program in the environment file, which
    // is bookkeeping, not a list the stack consults).
    let mut hooks: Vec<(Box<dyn HookSetup>, bool)> = Vec::new();
    if let Some(dir) = flags.get("hook-dir") {
        let dir = PathBuf::from(dir);
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-hook")
            .to_string();
        let out = PathBuf::from("target/hook-build").join(&name);
        let built = hook::build_dir(&dir, &out)?;
        let [so] = built.as_slice() else {
            return Err(format!(
                "{} produced {} artifacts; build it yourself and pass --setup with a program_id",
                dir.display(),
                built.len()
            ));
        };
        // The deployment is written to the environment file only with `--record`, like evidence.
        let program = hook::deploy_into(&mut env, flags, so, &name).await?;
        if flags.has("record") {
            env.save(env_path).map_err(|e| e.to_string())?;
        }
        let setup = flags
            .get("setup")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("setup.json"));
        println!(
            "hook {name} at {program}, set up from {}\n",
            setup.display()
        );
        hooks.push((
            Box::new(
                GenericExternalHook::from_file_for(&setup, Some(program))
                    .map_err(|e| format!("{}: {e}", setup.display()))?,
            ),
            false,
        ));
    } else if let Some(setup) = flags.get("setup") {
        let hook = GenericExternalHook::from_file(setup).map_err(|e| format!("{setup}: {e}"))?;
        let known = table::is_registered(&env, &hook.program_id());
        hooks.push((Box::new(hook), known));
    } else {
        let which = flags.get("hook").unwrap_or("all");
        for (name, env_key) in SHIPPED {
            let present = env_key.map_or(true, |key| env.template_program(key).is_ok());
            if which == *name || (which == "all" && present) {
                hooks.push((hook::provider(name, flags, &env)?, true));
            }
        }
        if hooks.is_empty() {
            return Err(format!(
                "unknown or unavailable --hook `{which}`; one of: all, {}",
                SHIPPED
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    let second = flags
        .get("second-hook")
        .map(|name| hook::provider(name, flags, &env))
        .transpose()?;
    let transfer_fee_bps: u16 = flags.number("transfer-fee-bps", 0)?;

    let mut amms = Vec::new();
    if matches!(amm, "cpmm" | "all") {
        amms.push("cpmm");
    }
    if matches!(amm, "clmm" | "all") {
        amms.push("clmm");
    }
    if amms.is_empty() {
        return Err(format!("unknown --amm `{amm}`: cpmm, clmm or all"));
    }
    let keep_state = flags.get("keep-state");
    if keep_state.is_some() && amms.len() * hooks.len() != 1 {
        return Err("--keep-state needs exactly one flow: one --amm and one --hook".into());
    }

    println!(
        "RAYDIUM TRANSFER HOOK E2E  environment {} ({})  wallet {}\n",
        env.name,
        env.cluster,
        payer.pubkey()
    );
    let mut chain = RpcChain::new(env.rpc_url.clone(), payer);
    let mut results: Vec<(String, Result<Vec<Evidence>, String>)> = Vec::new();
    for amm in &amms {
        for (hook, known) in &hooks {
            let label = format!("{amm} + {}", hook.name());
            println!("== {label}");
            let mut inputs = FlowInputs::new(&env, hook.as_ref(), fee_receiver.as_ref())
                .with_transfer_fee(transfer_fee_bps);
            if let Some(second) = &second {
                inputs = inputs.with_second_hook(second.as_ref());
            }
            if flags.has("exact-output") && *amm == "cpmm" {
                inputs = inputs.with_exact_output();
            }
            if flags.has("liquidity") && *amm == "cpmm" {
                inputs = inputs.with_liquidity();
            }
            let run = match *amm {
                "cpmm" => run_cpmm_session(&mut chain, &inputs).await,
                _ => run_clmm_session(&mut chain, &inputs).await,
            }
            .map_err(|e| e.to_string());
            let result = match run {
                Ok((evidence, session)) => {
                    print_table(
                        &chain,
                        &env,
                        amm,
                        hook.as_ref(),
                        *known,
                        &session,
                        &evidence,
                    )
                    .await;
                    if let Some(path) = keep_state {
                        session.save(path).map_err(|e| e.to_string())?;
                        println!(
                            "pool kept in {path}: try `raydium-hook {amm} swap --state {path}`"
                        );
                    }
                    Ok(evidence)
                }
                Err(message) => Err(message),
            };
            results.push((label, result));
        }
    }

    println!("\nRESULT");
    let mut failed = false;
    for (label, result) in &results {
        match result {
            Ok(_) => println!("  PASS  {label}"),
            Err(message) => {
                failed = true;
                println!("  FAIL  {label}\n          {message}");
            }
        }
    }
    if flags.has("record") {
        for (_, result) in &results {
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

/// The per-flow results table: every PASS comes from recorded evidence or the hooked mint's
/// on-chain state after the flow.
async fn print_table(
    chain: &RpcChain,
    env: &raydium_hook_driver::Environment,
    amm: &str,
    hook: &dyn HookSetup,
    known_to_the_repository: bool,
    session: &Session,
    evidence: &[Evidence],
) {
    let program = hook.program_id();
    let readiness = match session.mint_0() {
        Ok(mint) => inspect_readiness(&chain.reader(), mint).await.ok(),
        Err(_) => None,
    };
    let subject = Subject {
        amm,
        hook_name: hook.name(),
        program,
        readiness: readiness.as_ref(),
        known_to_the_repository,
    };
    let rows = table::rows(&subject, evidence);
    println!(
        "\n{}",
        table::render(&subject, &rows, evidence, &|sig| explorer(env, sig))
    );
}
