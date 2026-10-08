//! `ui-fixture`: leave a live Fair Launch pool and a funded wallet behind, for the browser UI and its
//! end-to-end test. The wallet's key never touches this command: only its public key is given.

use std::{fs, path::Path};

use raydium_hook_driver::{
    run_clmm_session, run_cpmm_session, Chain, Environment, FlowInputs, Session, UiFixture,
};
use solana_sdk::signature::Signer;

use super::{hook, load_env, rpc_chain};
use crate::args::{Flags, Res};

pub(crate) async fn ui_fixture(flags: &Flags) -> Res<()> {
    let (_, env): (String, Environment) = load_env(flags)?;
    let wallet = flags.pubkey("wallet")?;
    let out = flags.need("out")?;
    let mut chain = rpc_chain(&env, flags)?;
    let fee_receiver = flags
        .get("fee-receiver-keypair")
        .map(crate::args::keypair)
        .transpose()?;
    let provider = hook::provider(flags.get("hook").unwrap_or("fair-launch"), flags, &env)?;

    let fixture = UiFixture {
        wallet,
        seed_amount: flags.number("seed-amount", 1_000_000_000u64)?,
        wallet_hooked_amount: flags.number("wallet-hooked-amount", 100_000_000u64)?,
        wallet_quote_amount: flags.number("wallet-quote-amount", 1_000_000_000u64)?,
        wallet_lamports: flags.number("wallet-lamports", 2_000_000_000u64)?,
    };
    let extra_pools: u16 = flags.number("extra-pools", 0u16)?;
    if extra_pools > 0 && flags.get("amm").unwrap_or("cpmm") != "cpmm" {
        return Err("--extra-pools is for --amm cpmm".into());
    }
    let inputs = FlowInputs::new(&env, provider.as_ref(), fee_receiver.as_ref())
        .with_ui_fixture(fixture)
        .with_extra_pools(extra_pools);
    println!(
        "UI FIXTURE  environment {} ({})  payer {}  wallet {}",
        env.name,
        env.cluster,
        chain.payer().pubkey(),
        wallet
    );
    let amm = flags.get("amm").unwrap_or("cpmm");
    let (_, session) = match amm {
        "cpmm" => run_cpmm_session(&mut chain, &inputs).await,
        "clmm" => run_clmm_session(&mut chain, &inputs).await,
        other => return Err(format!("unknown --amm `{other}`: cpmm or clmm")),
    }
    .map_err(|e| e.to_string())?;
    write_fixture(out, &env, &session, &wallet.to_string())?;
    println!("pool and wallet details written to {out}");
    Ok(())
}

fn write_fixture(path: &str, env: &Environment, session: &Session, wallet: &str) -> Res<()> {
    let hook_program = session
        .hooks
        .first()
        .map(|hook| hook.program.clone())
        .unwrap_or_default();
    let body = serde_json::json!({
        "environment": env.name,
        "amm": session.amm,
        "rpc_url": env.rpc_url,
        "pool": session.pool,
        "extra_pools": session.extra_pools,
        "hooked_mint": session.mint_0,
        "quote_mint": session.mint_1,
        "hook_program": hook_program,
        "wallet": wallet,
        "hooked_account": session.accounts[0],
        "quote_account": session.accounts[1],
    });
    if let Some(parent) = Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("write {path}: {e}"))
}
