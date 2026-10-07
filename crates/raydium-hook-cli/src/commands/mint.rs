//! `mint create`: a Token-2022 mint (optionally hooked or with a transfer fee), an account for the
//! payer, and optionally a first supply.

use raydium_hook_driver::{
    chain::Chain,
    inspect_readiness,
    token::{
        create_mint_instructions, create_token_account_instructions, mint_to_instruction,
        MintFeatures,
    },
};
use solana_sdk::signature::{Keypair, Signer};
use spl_token_2022::extension::transfer_hook::instruction as transfer_hook_instruction;

use super::{explorer, inspect, load_env, rpc_chain};
use crate::args::{Flags, Res};

/// `mint create --env FILE --keypair FILE [--decimals N] [--hook PROGRAM | --hookable]
/// [--transfer-fee-bps N] [--supply N]`
pub(crate) async fn create(flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    let mut chain = rpc_chain(&env, flags)?;
    let payer = chain.payer().pubkey();
    let decimals = flags.number("decimals", 6u8)?;
    let hook = flags.pubkey_opt("hook")?;
    let features = MintFeatures {
        // The extension is needed to point a hook at the mint now or later.
        hook: hook.is_some() || flags.has("hookable"),
        transfer_fee_bps: flags.number("transfer-fee-bps", 0u16)?,
    };
    let supply = flags.number("supply", 0u64)?;

    let mint = Keypair::new();
    let account = Keypair::new();
    let mut instructions = create_mint_instructions(&payer, &mint, &payer, decimals, features);
    if let Some(program) = hook {
        // Pointing the hook at the mint right away. (Raydium's liquidity paths reject hooked
        // mints, so for a pool create the mint with `--hookable` and attach the hook with
        // `hook setup` after the pool has liquidity.)
        instructions.push(
            transfer_hook_instruction::update(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &payer,
                &[],
                Some(program),
            )
            .map_err(|e| e.to_string())?,
        );
    }
    instructions.extend(create_token_account_instructions(
        &payer,
        &account,
        &mint.pubkey(),
        &payer,
        features,
    ));
    if supply > 0 {
        instructions.push(mint_to_instruction(
            &mint.pubkey(),
            &account.pubkey(),
            &payer,
            supply,
        ));
    }
    let sent = chain
        .send(&instructions, &[&mint, &account])
        .await
        .map_err(|e| format!("creating the mint failed: {e}"))?;
    println!("mint     {}", mint.pubkey());
    println!(
        "account  {} (owned by {payer}, holds {supply})",
        account.pubkey()
    );
    println!("  {}", explorer(&env, &sent.signature));
    if features.hook {
        println!();
        let readiness = inspect_readiness(&chain.reader(), mint.pubkey())
            .await
            .map_err(|e| e.to_string())?;
        inspect::print_readiness(&readiness);
    }
    Ok(())
}
