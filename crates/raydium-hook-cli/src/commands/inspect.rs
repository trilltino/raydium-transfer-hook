//! `inspect`: read a mint's Transfer Hook and the transport facts around it.

use std::str::FromStr;

use raydium_hook_driver::{chain::Chain, RpcChain};
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use spl_token_2022::{
    extension::{
        transfer_hook::{get_program_id, TransferHook},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::Mint,
};

use crate::args::{Flags, Res, USAGE};

pub(crate) async fn inspect(flags: &Flags) -> Res<()> {
    let rpc = flags.need("rpc")?;
    let mint_text = flags
        .positional
        .first()
        .ok_or_else(|| format!("give a mint address\n\n{USAGE}"))?;
    let mint = Pubkey::from_str(mint_text).map_err(|e| format!("bad mint: {e}"))?;
    let chain = RpcChain::new(rpc.to_string(), Keypair::new());
    let reader = chain.reader();
    let account = reader(mint)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("mint account not found")?;
    println!("mint            {mint}");
    println!("owner program   {}", account.owner);
    if account.owner != spl_token_2022::id() {
        println!("not a Token-2022 mint: no Transfer Hook possible");
        return Ok(());
    }
    let state = StateWithExtensions::<Mint>::unpack(&account.data).map_err(|e| e.to_string())?;
    let Ok(extension) = state.get_extension::<TransferHook>() else {
        println!("Transfer Hook   none (no TransferHook extension)");
        return Ok(());
    };
    let hook = get_program_id(&state);
    let authority = Option::<Pubkey>::from(extension.authority);
    println!(
        "hook authority  {}",
        authority
            .map(|a| a.to_string())
            .unwrap_or_else(|| "revoked (hook cannot be re-pointed)".into())
    );
    let Some(hook) = hook else {
        println!("hook program    not set");
        return Ok(());
    };
    println!("hook program    {hook}");
    let program = reader(hook).await.map_err(|e| e.to_string())?;
    match &program {
        None => println!("  TRANSPORT: hook program account does not exist"),
        Some(p) if !p.executable => println!("  TRANSPORT: hook program is not executable"),
        Some(p) => {
            println!("  loader        {}", p.owner);
            if p.owner == solana_sdk::bpf_loader_upgradeable::id() && p.data.len() >= 36 {
                let data_address = Pubkey::try_from(&p.data[4..36]).map_err(|e| e.to_string())?;
                if let Some(programdata) = reader(data_address).await.map_err(|e| e.to_string())? {
                    let upgrade =
                        if programdata.data.get(12) == Some(&1) && programdata.data.len() >= 45 {
                            Pubkey::try_from(&programdata.data[13..45])
                                .map(|k| k.to_string())
                                .unwrap_or_default()
                        } else {
                            "none (immutable)".into()
                        };
                    println!("  upgrade auth  {upgrade}  (can replace the rule for every token)");
                }
            }
        }
    }
    let list = spl_transfer_hook_interface::get_extra_account_metas_address(&mint, &hook);
    match reader(list).await.map_err(|e| e.to_string())? {
        Some(l) if l.owner == hook => {
            println!("  validation list {list}  present, owned by the hook")
        }
        Some(l) => println!(
            "  TRANSPORT: validation list owned by {} not the hook",
            l.owner
        ),
        None => println!("  TRANSPORT: validation list {list} does not exist; transfers will fail"),
    }
    println!(
        "\nTransport checks only. They do not say whether the hook's own settings are initialised,\n\
         whether its rule is sensible, or who controls those settings."
    );
    Ok(())
}
