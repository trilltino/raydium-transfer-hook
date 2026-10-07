//! One module per command.

pub(crate) mod deploy;
pub(crate) mod e2e;
pub(crate) mod inspect;

pub(crate) fn explorer(env: &raydium_hook_driver::Environment, signature: &str) -> String {
    match env.cluster.as_str() {
        "devnet" => format!("https://explorer.solana.com/tx/{signature}?cluster=devnet"),
        "mainnet-beta" => format!("https://explorer.solana.com/tx/{signature}"),
        _ => signature.to_string(),
    }
}
