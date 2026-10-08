//! `hook build | deploy | setup | inspect`: the author's loop for a hook of their own.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use raydium_hook_driver::{
    chain::Chain, inspect_readiness, ArbitraryHook, CreatorCommitmentHook, Environment,
    FairLaunchHook, GenericExternalHook, HolderRewardsHook, HookContext, HookSetup, ReferenceHook,
};
use sha2::{Digest, Sha256};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
};

use super::{
    deploy::{deploy_program, toolchain, Program},
    explorer, inspect, load_env, rpc_chain,
};
use crate::args::{Flags, Res};

/// `hook build DIR [--out DIR]`
pub(crate) fn build(flags: &Flags) -> Res<()> {
    let dir = PathBuf::from(flags.first_positional("the hook's directory")?);
    let out = PathBuf::from(flags.get("out").unwrap_or("target/hook-build"));
    build_dir(&dir, &out)?;
    println!(
        "\nnext: raydium-hook hook deploy --env ENV --keypair KEY --so <artifact> --name NAME"
    );
    Ok(())
}

/// Build the hook in `dir` with `cargo build-sbf` into `out` and return the fresh artifacts.
pub(crate) fn build_dir(dir: &Path, out: &Path) -> Res<Vec<PathBuf>> {
    let manifest = dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!("{} does not exist", manifest.display()));
    }
    fs::create_dir_all(out).map_err(|e| format!("create {}: {e}", out.display()))?;
    let started = SystemTime::now();
    println!("building {} ...", manifest.display());
    let status = Command::new("cargo")
        .args(["build-sbf", "--manifest-path"])
        .arg(&manifest)
        .arg("--sbf-out-dir")
        .arg(out)
        .status()
        .map_err(|e| {
            format!("could not run `cargo build-sbf`: {e} (is the Solana toolchain installed?)")
        })?;
    if !status.success() {
        return Err("`cargo build-sbf` failed".into());
    }
    let mut built = Vec::new();
    for entry in fs::read_dir(out).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|modified| modified >= started)
            .unwrap_or(false);
        if path.extension().is_some_and(|e| e == "so") && fresh {
            built.push(path);
        }
    }
    if built.is_empty() {
        return Err(format!("no .so was produced in {}", out.display()));
    }
    for so in &built {
        let bytes = fs::read(so).map_err(|e| e.to_string())?;
        println!("\nartifact  {}", so.display());
        println!("  bytes   {}", bytes.len());
        println!("  sha256  {:x}", Sha256::digest(&bytes));
        let stem = so.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let keypair_path = out.join(format!("{stem}-keypair.json"));
        if let Ok(keypair) = read_keypair_file(&keypair_path) {
            println!(
                "  program id {} (from {}, made by `cargo build-sbf`)",
                keypair.pubkey(),
                keypair_path.display()
            );
        }
    }
    Ok(built)
}

/// The program keypair to deploy under: given, else under `--keys`, else the one `cargo build-sbf`
/// left beside the artifact, else a new one under `--keys`.
fn program_keypair(flags: &Flags, so: &Path, name: &str) -> Res<(PathBuf, bool)> {
    if let Some(path) = flags.get("program-keypair") {
        return Ok((PathBuf::from(path), false));
    }
    let keys = PathBuf::from(flags.get("keys").unwrap_or(".keys"));
    let in_keys = keys.join(format!("{name}.json"));
    if in_keys.exists() {
        return Ok((in_keys, false));
    }
    let stem = so.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let beside = so.with_file_name(format!("{stem}-keypair.json"));
    if beside.exists() {
        return Ok((beside, false));
    }
    fs::create_dir_all(&keys).map_err(|e| format!("create {}: {e}", keys.display()))?;
    write_keypair_file(&Keypair::new(), &in_keys).map_err(|e| e.to_string())?;
    Ok((in_keys, true))
}

/// `hook deploy --env FILE --keypair FILE --so FILE --name NAME`
pub(crate) async fn deploy(flags: &Flags) -> Res<()> {
    let (env_path, mut env) = load_env(flags)?;
    let so = PathBuf::from(flags.need("so")?);
    let name = flags.need("name")?;
    let program = deploy_into(&mut env, flags, &so, name).await?;
    env.save(&env_path).map_err(|e| e.to_string())?;
    println!(
        "\nprogram id {program} recorded in {env_path} as `{}`",
        name.replace('-', "_")
    );
    Ok(())
}

/// Deploy the artifact `so` as `name` (`--keypair` pays; the program keypair comes from
/// `--program-keypair`, `--keys` or beside the artifact) and record it in `env` (not saved).
pub(crate) async fn deploy_into(
    env: &mut Environment,
    flags: &Flags,
    so: &Path,
    name: &str,
) -> Res<Pubkey> {
    let deployer_path = flags.need("keypair")?;
    let (keypair_path, created) = program_keypair(flags, so, name)?;
    let program_keypair = read_keypair_file(&keypair_path)
        .map_err(|e| format!("cannot read {}: {e}", keypair_path.display()))?;
    if created {
        println!("made a new program keypair at {}", keypair_path.display());
    }
    let program = program_keypair.pubkey();
    let toolchain = toolchain();
    let deployed = deploy_program(
        env,
        deployer_path,
        &Program {
            name,
            so,
            program_keypair: &keypair_path,
            program,
            source: format!("local build of {}", so.display()),
            lockfile: None,
        },
        toolchain.as_deref(),
    )
    .await?;
    if let Some(deployment) = deployed {
        env.deployments.retain(|d| d.name != name);
        env.deployments.push(deployment);
    }
    env.programs
        .templates
        .insert(name.replace('-', "_"), program.to_string());
    Ok(program)
}

/// The program id for a hook `kind`: `--program`, else the environment's.
fn program_for(kind: &str, flags: &Flags, env: &Environment) -> Res<Pubkey> {
    if let Some(program) = flags.pubkey_opt("program")? {
        return Ok(program);
    }
    let from_env = match kind {
        "reference" => env.reference_hook_program(),
        "arbitrary" => env.arbitrary_hook_program(),
        "creator-commitment" => env.template_program("creator_commitment"),
        "fair-launch" | "fair-launch-per-slot" => env.template_program("fair_launch"),
        "holder-rewards" | "holder-rewards-one-time" => env.template_program("holder_rewards"),
        other => return Err(format!("unknown --kind `{other}`")),
    };
    from_env.map_err(|e| format!("{e}; give --program"))
}

/// Build the setup provider for `kind` from the flags.
pub(crate) fn provider(kind: &str, flags: &Flags, env: &Environment) -> Res<Box<dyn HookSetup>> {
    if kind == "generic" {
        let path = flags.need("setup")?;
        return Ok(Box::new(
            GenericExternalHook::from_file(path).map_err(|e| e.to_string())?,
        ));
    }
    let program = program_for(kind, flags, env)?;
    Ok(match kind {
        "reference" => Box::new(ReferenceHook {
            program_id: program,
            max_transfer: flags.number("max-transfer", 500u64)?,
        }),
        "arbitrary" => Box::new(ArbitraryHook {
            program_id: program,
            max_per_slot: flags.number("max-per-slot", 2u32)?,
        }),
        "creator-commitment" => Box::new(CreatorCommitmentHook::new(
            program,
            flags.number("vest-seconds", 120i64)?,
        )),
        "fair-launch" => {
            let mut hook = FairLaunchHook::new(program, flags.number("window-seconds", 150i64)?);
            hook.max_buy = flags.number("max-buy", hook.max_buy)?;
            hook.max_wallet = flags.number("max-wallet", hook.max_wallet)?;
            hook.max_buys_per_slot = flags.number("max-buys-per-slot", hook.max_buys_per_slot)?;
            hook.max_priority_micro_lamports =
                flags.number("max-priority", hook.max_priority_micro_lamports)?;
            Box::new(hook)
        }
        "fair-launch-per-slot" => Box::new(FairLaunchHook::per_slot_only(program)),
        "holder-rewards" => Box::new(HolderRewardsHook::new(
            program,
            flags.number("reward-seconds", 100u32)?,
        )),
        "holder-rewards-one-time" => Box::new(HolderRewardsHook::one_time(
            program,
            flags.number("reward-seconds", 100u32)?,
        )),
        other => return Err(format!("unknown --kind `{other}`")),
    })
}

/// Unix time on the cluster, from the Clock sysvar.
async fn cluster_time<C: Chain>(chain: &mut C) -> Res<i64> {
    let clock = chain
        .account(&solana_sdk::sysvar::clock::id())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("the Clock sysvar is missing")?;
    clock
        .data
        .get(32..40)
        .and_then(|b| b.try_into().ok())
        .map(i64::from_le_bytes)
        .ok_or_else(|| "the Clock sysvar is malformed".to_string())
}

/// `hook setup --env FILE --keypair FILE --kind KIND --mint MINT ...`
pub(crate) async fn setup(flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    let mut chain = rpc_chain(&env, flags)?;
    let kind = flags.need("kind")?;
    let mint = flags.pubkey("mint")?;
    let hook = provider(kind, flags, &env)?;

    // The context a provider needs. Only what the kind uses has to be given.
    let need_vault = matches!(
        kind,
        "fair-launch" | "fair-launch-per-slot" | "holder-rewards" | "holder-rewards-one-time"
    );
    let pool_vault = flags.pubkey_opt("pool-vault")?;
    if need_vault && pool_vault.is_none() {
        return Err(format!(
            "`{kind}` needs --pool-vault (the pool's vault of the hooked mint)"
        ));
    }
    let creator_account = flags.pubkey_opt("creator-account")?;
    if kind == "creator-commitment" && creator_account.is_none() {
        return Err(
            "`creator-commitment` needs --creator-account (the account whose balance vests)".into(),
        );
    }
    let reward_mint = flags.pubkey_opt("reward-mint")?;
    if matches!(kind, "holder-rewards" | "holder-rewards-one-time") && reward_mint.is_none() {
        return Err(format!("`{kind}` needs --reward-mint (the token paid out)"));
    }
    let payer = chain.payer().pubkey();
    let ctx = HookContext {
        payer,
        hooked_mint: mint,
        quote_mint: reward_mint.unwrap_or_default(),
        trader_accounts: [creator_account.unwrap_or_default(), Pubkey::default()],
        pool_authority: Pubkey::default(),
        vaults: [pool_vault.unwrap_or_default(), Pubkey::default()],
        now: cluster_time(&mut chain).await?,
    };
    let sent = chain
        .send(&hook.enable_instructions(&ctx), &[])
        .await
        .map_err(|e| format!("setting up the hook failed: {e}"))?;
    println!(
        "set up {} on mint {mint}\n  {}",
        hook.name(),
        explorer(&env, &sent.signature)
    );
    let readiness = inspect_readiness(&chain.reader(), mint)
        .await
        .map_err(|e| e.to_string())?;
    println!();
    inspect::print_readiness(&readiness);
    Ok(())
}

/// `hook inspect MINT --rpc URL`: the same as `inspect`.
pub(crate) async fn inspect_hook(flags: &Flags) -> Res<()> {
    inspect::inspect(flags).await
}
