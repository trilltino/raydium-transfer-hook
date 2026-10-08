//! `deploy`: put the integration programs on a cluster and record each deployment. Also the shared
//! core that `hook deploy` uses for a single, arbitrary program.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

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
    /// The `Cargo.lock` the artifact is built with, relative to the repository root.
    lockfile: &'static str,
}

pub(crate) const ARTIFACTS: &[Artifact] = &[
    Artifact {
        name: "reference-hook",
        file: "transfer_hook_starter.so",
        keypair: "hook-program.json",
        program_id: |e| e.programs.reference_hook.as_ref(),
        source_key: "reference_hook",
        lockfile: "Cargo.lock",
    },
    Artifact {
        name: "arbitrary-test-hook",
        file: "arbitrary_test_hook.so",
        keypair: "arbitrary-hook-program.json",
        program_id: |e| e.programs.arbitrary_hook.as_ref(),
        source_key: "arbitrary_hook",
        lockfile: "Cargo.lock",
    },
    Artifact {
        name: "creator-commitment",
        file: "creator_commitment_hook.so",
        keypair: "creator-commitment-program.json",
        program_id: |e| e.programs.templates.get("creator_commitment"),
        source_key: "creator_commitment",
        lockfile: "Cargo.lock",
    },
    Artifact {
        name: "fair-launch",
        file: "fair_launch_hook.so",
        keypair: "fair-launch-program.json",
        program_id: |e| e.programs.templates.get("fair_launch"),
        source_key: "fair_launch",
        lockfile: "Cargo.lock",
    },
    Artifact {
        name: "holder-rewards",
        file: "holder_rewards_hook.so",
        keypair: "holder-rewards-program.json",
        program_id: |e| e.programs.templates.get("holder_rewards"),
        source_key: "holder_rewards",
        lockfile: "Cargo.lock",
    },
    Artifact {
        name: "cpmm",
        file: "raydium_cp_swap.so",
        keypair: "cpmm-program.json",
        program_id: |e| e.programs.cpmm.as_ref(),
        source_key: "cpmm_hook",
        lockfile: "target/upstream/cpmm-hook/Cargo.lock",
    },
    Artifact {
        name: "clmm",
        file: "raydium_clmm.so",
        keypair: "clmm-program.json",
        program_id: |e| e.programs.clmm.as_ref(),
        source_key: "clmm_hook",
        lockfile: "target/upstream/clmm-hook/Cargo.lock",
    },
];

/// The build toolchain, as `cargo build-sbf --version` reports it, on one line. `None` if it is
/// not installed (the artifact may still have been built elsewhere).
pub(crate) fn toolchain() -> Option<String> {
    let output = Command::new("cargo")
        .args(["build-sbf", "--version"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (output.status.success() && !line.is_empty()).then_some(line)
}

/// SHA-256 of a lockfile, if it exists.
pub(crate) fn lockfile_sha256(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}

/// Everything needed to deploy and record one program.
pub(crate) struct Program<'a> {
    pub(crate) name: &'a str,
    pub(crate) so: &'a Path,
    pub(crate) program_keypair: &'a Path,
    pub(crate) program: Pubkey,
    pub(crate) source: String,
    pub(crate) lockfile: Option<&'a Path>,
}

/// Deploy `program` with `solana program deploy`, skipping it if it is already deployed, and
/// return the record. `Ok(None)` means it was already there.
pub(crate) async fn deploy_program(
    env: &Environment,
    deployer_path: &str,
    program: &Program<'_>,
    toolchain: Option<&str>,
) -> Res<Option<Deployment>> {
    let deployer = keypair(deployer_path)?;
    let chain = RpcChain::new(env.rpc_url.clone(), keypair(deployer_path)?);
    let reader = chain.reader();
    let bytes =
        fs::read(program.so).map_err(|e| format!("cannot read {}: {e}", program.so.display()))?;
    let sha = format!("{:x}", Sha256::digest(&bytes));

    if let Some(existing) = reader(program.program).await.map_err(|e| e.to_string())? {
        if existing.executable {
            println!(
                "{}: {} already deployed, skipping (use `solana program deploy` to upgrade)",
                program.name, program.program
            );
            return Ok(None);
        }
    }
    let on_disk = keypair(&program.program_keypair.to_string_lossy())?;
    if on_disk.pubkey() != program.program {
        return Err(format!(
            "{} does not match the program id {}",
            program.program_keypair.display(),
            program.program
        ));
    }
    println!(
        "{}: deploying {} bytes (sha256 {sha}) to {} ...",
        program.name,
        bytes.len(),
        program.program
    );
    let output = Command::new("solana")
        .arg("program")
        .arg("deploy")
        .arg(program.so)
        .args(["--url", &env.rpc_url])
        .args(["--keypair", deployer_path])
        .args([
            "--program-id",
            program.program_keypair.to_str().unwrap_or_default(),
        ])
        .args(["--upgrade-authority", deployer_path])
        .output()
        .map_err(|e| format!("could not run `solana`: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(format!(
            "`solana program deploy` failed for {}:\n{stdout}\n{stderr}",
            program.name
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
    Ok(Some(Deployment {
        name: program.name.to_string(),
        program_id: program.program.to_string(),
        signature,
        artifact_sha256: sha,
        artifact_bytes: bytes.len() as u64,
        upgrade_authority: deployer.pubkey().to_string(),
        source: program.source.clone(),
        lockfile_sha256: program.lockfile.and_then(lockfile_sha256),
        toolchain: toolchain.map(str::to_string),
    }))
}

pub(crate) async fn deploy(flags: &Flags) -> Res<()> {
    let env_path = flags.need("env")?;
    let mut env = Environment::load(env_path).map_err(|e| e.to_string())?;
    let deployer_path = flags.need("keypair")?;
    let artifacts = PathBuf::from(flags.need("artifacts")?);
    let keys = PathBuf::from(flags.need("keys")?);
    let only = flags.get("only");
    let toolchain = toolchain();

    for artifact in ARTIFACTS {
        if only.is_some_and(|n| n != artifact.name) {
            continue;
        }
        // An environment need not have every program (a fork may deploy only some).
        let Some(program_text) = (artifact.program_id)(&env).cloned() else {
            if only.is_some() {
                return Err(format!(
                    "environment has no program id for {}",
                    artifact.name
                ));
            }
            println!("{}: not in this environment, skipping", artifact.name);
            continue;
        };
        let program = Pubkey::from_str(&program_text).map_err(|e| e.to_string())?;
        let program_keypair = keys.join(artifact.keypair);
        let so = artifacts.join(artifact.file);
        let lockfile = Path::new(artifact.lockfile);
        let source = env
            .sources
            .get(artifact.source_key)
            .cloned()
            .unwrap_or_default();
        let deployed = deploy_program(
            &env,
            deployer_path,
            &Program {
                name: artifact.name,
                so: &so,
                program_keypair: &program_keypair,
                program,
                source,
                lockfile: Some(lockfile),
            },
            toolchain.as_deref(),
        )
        .await?;
        if let Some(deployment) = deployed {
            env.deployments.retain(|d| d.name != artifact.name);
            env.deployments.push(deployment);
            env.save(env_path).map_err(|e| e.to_string())?;
        }
    }
    println!("environment {env_path} updated");
    Ok(())
}
