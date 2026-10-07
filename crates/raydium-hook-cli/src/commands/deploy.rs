//! `deploy`: put the integration programs on a cluster and record each deployment.

use std::{fs, path::PathBuf, process::Command, str::FromStr};

use raydium_hook_driver::{chain::Chain, env::Deployment, Environment, RpcChain};
use sha2::{Digest, Sha256};
use solana_sdk::{pubkey::Pubkey, signature::Signer};

use crate::args::{keypair, Flags, Res};

pub(crate) struct Artifact {
    name: &'static str,
    file: &'static str,
    keypair: &'static str,
    program_id: fn(&Environment) -> Option<&String>,
    source_key: &'static str,
}

pub(crate) const ARTIFACTS: &[Artifact] = &[
    Artifact {
        name: "reference-hook",
        file: "reference_hook_onchain.so",
        keypair: "hook-program.json",
        program_id: |e| e.programs.reference_hook.as_ref(),
        source_key: "reference_hook",
    },
    Artifact {
        name: "arbitrary-test-hook",
        file: "arbitrary_test_hook.so",
        keypair: "arbitrary-hook-program.json",
        program_id: |e| e.programs.arbitrary_hook.as_ref(),
        source_key: "arbitrary_hook",
    },
    Artifact {
        name: "cpmm",
        file: "raydium_cp_swap.so",
        keypair: "cpmm-program.json",
        program_id: |e| e.programs.cpmm.as_ref(),
        source_key: "cpmm_hook",
    },
    Artifact {
        name: "clmm",
        file: "raydium_clmm.so",
        keypair: "clmm-program.json",
        program_id: |e| e.programs.clmm.as_ref(),
        source_key: "clmm_hook",
    },
];

pub(crate) async fn deploy(flags: &Flags) -> Res<()> {
    let env_path = flags.need("env")?;
    let mut env = Environment::load(env_path).map_err(|e| e.to_string())?;
    let deployer_path = flags.need("keypair")?;
    let deployer = keypair(deployer_path)?;
    let artifacts = PathBuf::from(flags.need("artifacts")?);
    let keys = PathBuf::from(flags.need("keys")?);
    let only = flags.get("only");
    let chain = RpcChain::new(env.rpc_url.clone(), keypair(deployer_path)?);
    let reader = chain.reader();

    for artifact in ARTIFACTS {
        if only.is_some_and(|n| n != artifact.name) {
            continue;
        }
        let program_text = (artifact.program_id)(&env)
            .ok_or_else(|| format!("environment has no program id for {}", artifact.name))?
            .clone();
        let program = Pubkey::from_str(&program_text).map_err(|e| e.to_string())?;
        let program_keypair = keys.join(artifact.keypair);
        let so = artifacts.join(artifact.file);
        let bytes = fs::read(&so).map_err(|e| format!("cannot read {}: {e}", so.display()))?;
        let sha = format!("{:x}", Sha256::digest(&bytes));

        if let Some(existing) = reader(program).await.map_err(|e| e.to_string())? {
            if existing.executable {
                println!(
                    "{}: {program} already deployed, skipping (use `solana program deploy` to upgrade)",
                    artifact.name
                );
                continue;
            }
        }
        let on_disk = keypair(program_keypair.to_str().unwrap_or_default())?;
        if on_disk.pubkey() != program {
            return Err(format!(
                "{} does not match the environment's program id {program}",
                program_keypair.display()
            ));
        }
        println!(
            "{}: deploying {} bytes (sha256 {sha}) to {program} ...",
            artifact.name,
            bytes.len()
        );
        let output = Command::new("solana")
            .arg("program")
            .arg("deploy")
            .arg(&so)
            .args(["--url", &env.rpc_url])
            .args(["--keypair", deployer_path])
            .args(["--program-id", program_keypair.to_str().unwrap_or_default()])
            .args(["--upgrade-authority", deployer_path])
            .output()
            .map_err(|e| format!("could not run `solana`: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(format!(
                "`solana program deploy` failed for {}:\n{stdout}\n{stderr}",
                artifact.name
            ));
        }
        let signature = stdout
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("Signature:")
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_default();
        println!("  deployed. {stdout}");
        env.deployments.retain(|d| d.name != artifact.name);
        env.deployments.push(Deployment {
            name: artifact.name.to_string(),
            program_id: program_text,
            signature,
            artifact_sha256: sha,
            artifact_bytes: bytes.len() as u64,
            upgrade_authority: deployer.pubkey().to_string(),
            source: env
                .sources
                .get(artifact.source_key)
                .cloned()
                .unwrap_or_default(),
        });
        env.save(env_path).map_err(|e| e.to_string())?;
    }
    println!("environment {env_path} updated");
    Ok(())
}
