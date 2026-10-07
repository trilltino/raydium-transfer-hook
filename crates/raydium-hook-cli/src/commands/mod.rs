//! One module per command.

pub(crate) mod deploy;
pub(crate) mod e2e;
pub(crate) mod hook;
pub(crate) mod inspect;
pub(crate) mod mint;
pub(crate) mod probe;
pub(crate) mod swap;
pub(crate) mod table;
pub(crate) mod template;

use raydium_hook_driver::{Environment, RpcChain};

use crate::args::{keypair, Flags, Res};

pub(crate) fn explorer(env: &Environment, signature: &str) -> String {
    match env.cluster.as_str() {
        "devnet" => format!("https://explorer.solana.com/tx/{signature}?cluster=devnet"),
        "mainnet-beta" => format!("https://explorer.solana.com/tx/{signature}"),
        _ => signature.to_string(),
    }
}

/// The environment named by `--env`, and its path (for commands that write it back).
pub(crate) fn load_env(flags: &Flags) -> Res<(String, Environment)> {
    let path = flags.need("env")?.to_string();
    let env = Environment::load(&path).map_err(|e| e.to_string())?;
    Ok((path, env))
}

/// An RPC chain for `env`, paying with the keypair at `--keypair`.
pub(crate) fn rpc_chain(env: &Environment, flags: &Flags) -> Res<RpcChain> {
    Ok(RpcChain::new(
        env.rpc_url.clone(),
        keypair(flags.need("keypair")?)?,
    ))
}
