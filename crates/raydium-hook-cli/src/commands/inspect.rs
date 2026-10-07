//! `inspect`: read a mint's Transfer Hook and the transport facts around it.

use std::str::FromStr;

use raydium_hook_driver::{
    chain::Chain,
    readiness::{inspect_readiness, Readiness, UpgradeInfo},
    RpcChain,
};
use solana_sdk::{pubkey::Pubkey, signature::Keypair};

use crate::args::{Flags, Res, USAGE};

/// Print a readiness report: what a transfer needs to reach the hook, and who holds which power.
pub(crate) fn print_readiness(readiness: &Readiness) {
    println!("mint            {}", readiness.mint);
    if !readiness.mint_exists {
        println!("  TRANSPORT: the mint account does not exist");
        return;
    }
    if !readiness.token_2022 {
        println!("not a Token-2022 mint: no Transfer Hook is possible");
        return;
    }
    if !readiness.has_hook_extension {
        println!("Transfer Hook   none (no TransferHook extension)");
        return;
    }
    println!(
        "hook authority  {}",
        readiness
            .hook_authority
            .map(|a| a.to_string())
            .unwrap_or_else(
                || "revoked (the mint can no longer be re-pointed at another hook)".into()
            )
    );
    let Some(hook) = readiness.hook_program else {
        println!("hook program    not set");
        return;
    };
    println!("hook program    {hook}");
    if let Some(program) = &readiness.program {
        if let Some(loader) = program.loader {
            println!("  loader          {loader}");
        }
        match program.upgrade {
            Some(UpgradeInfo::Authority(authority)) => println!(
                "  upgrade auth    {authority}  (can replace the rule for every mint using this hook)"
            ),
            Some(UpgradeInfo::Immutable) => {
                println!("  upgrade auth    none: the program is immutable")
            }
            Some(UpgradeInfo::NotApplicable(loader)) => {
                println!("  upgrade auth    not applicable (loader {loader})")
            }
            None => {}
        }
    }
    if let Some(list) = &readiness.validation_list {
        if list.exists {
            println!(
                "  validation list {}  owned by the hook: {}, Execute shape: {}, extra accounts: {}",
                list.address,
                list.owned_by_hook,
                list.has_execute_shape,
                list.extra_accounts
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".into())
            );
        } else {
            println!("  validation list {}  does not exist", list.address);
        }
    }
    let problems = readiness.transport_problems();
    if problems.is_empty() {
        println!("\nTRANSPORT READY: a transfer can reach the hook.");
    } else {
        println!();
        for problem in problems {
            println!("TRANSPORT PROBLEM: {problem}");
        }
    }
    println!(
        "\nTransport checks only. They do not say whether the hook's own settings are initialised,\n\
         whether its rule is sensible or can freeze transfers, or who controls those settings."
    );
}

pub(crate) async fn inspect(flags: &Flags) -> Res<()> {
    let rpc = flags.need("rpc")?;
    let mint_text = flags
        .positional
        .first()
        .ok_or_else(|| format!("give a mint address\n\n{USAGE}"))?;
    let mint = Pubkey::from_str(mint_text).map_err(|e| format!("bad mint: {e}"))?;
    let chain = RpcChain::new(rpc.to_string(), Keypair::new());
    let readiness = inspect_readiness(&chain.reader(), mint)
        .await
        .map_err(|e| e.to_string())?;
    print_readiness(&readiness);
    Ok(())
}
