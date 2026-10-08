//! `mint approve` and `mint approval`: let the person who runs a Raydium deployment approve hooked
//! mints for pool creation, and let anyone check whether a mint was approved.
//!
//! The work is in `raydium_hook_driver::approval`, the same code the end-to-end flows run, so this
//! command is exercised by every flow. Only the program's admin can approve (see that module).

use std::str::FromStr;

use raydium_hook_driver::{
    approval::{self, Amm, MintCheck, Outcome, RecordState},
    chain::Chain,
    inspect_readiness, RpcChain,
};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

use super::{explorer, load_env, rpc_chain};
use crate::args::{Flags, Res};

/// Every mint named by `--mint` (repeatable) and `--mints-file`, each once, in the order given.
fn mints_from(flags: &Flags) -> Res<Vec<Pubkey>> {
    let mut mints: Vec<Pubkey> = Vec::new();
    for text in flags.all("mint") {
        mints.push(Pubkey::from_str(text).map_err(|e| format!("--mint {text}: {e}"))?);
    }
    if let Some(file) = flags.get("mints-file") {
        let text = std::fs::read_to_string(file).map_err(|e| format!("cannot read {file}: {e}"))?;
        mints.extend(parse_mints_file(&text).map_err(|e| format!("{file}: {e}"))?);
    }
    let mut unique: Vec<Pubkey> = Vec::new();
    for mint in mints {
        if !unique.contains(&mint) {
            unique.push(mint);
        }
    }
    if unique.is_empty() {
        return Err("give at least one mint: --mint MINT (repeatable) or --mints-file FILE".into());
    }
    Ok(unique)
}

/// One mint address per line. Blank lines and `#` comments (whole-line or after the address) are
/// ignored.
pub(crate) fn parse_mints_file(text: &str) -> Result<Vec<Pubkey>, String> {
    let mut mints = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        mints.push(
            Pubkey::from_str(line).map_err(|e| format!("line {}: `{line}`: {e}", index + 1))?,
        );
    }
    Ok(mints)
}

fn amms_from(flags: &Flags) -> Res<Vec<Amm>> {
    Amm::parse_list(flags.get("amm").unwrap_or("all")).map_err(|e| e.to_string())
}

/// `mint approve --env FILE --keypair ADMIN.json (--mint M ... | --mints-file FILE)
/// [--amm cpmm|clmm|all] [--dry-run]`
pub(crate) async fn approve(flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    let mints = mints_from(flags)?;
    let amms = amms_from(flags)?;
    let dry_run = flags.has("dry-run");
    let mut chain = rpc_chain(&env, flags)?;
    println!(
        "approving {} mint(s) on {} as {}{}\n",
        mints.len(),
        amms.iter()
            .map(|a| a.name())
            .collect::<Vec<_>>()
            .join(" and "),
        chain.payer().pubkey(),
        if dry_run {
            " (dry run: nothing is sent)"
        } else {
            ""
        }
    );
    let rows = approval::approve(&mut chain, &env, &amms, &mints, dry_run)
        .await
        .map_err(|e| approval::redact(&e.to_string()))?;
    let mut problems = 0;
    for row in &rows {
        println!("{:<5} {}  {}", row.amm.name(), row.mint, row.outcome);
        if let Outcome::Approved { signature } = &row.outcome {
            println!("      {}", explorer(&env, signature));
        }
        if !row.note.is_empty() {
            println!("      note: {}", row.note);
        }
        if row.outcome.is_problem() {
            problems += 1;
        }
    }
    if problems > 0 {
        return Err(format!(
            "{problems} approval(s) did not go through; see above"
        ));
    }
    if dry_run {
        println!("\ndry run complete: the simulations passed and nothing was sent.");
    } else {
        println!(
            "\ndone. A pool can now be created with these mints; check any time with `mint approval`."
        );
    }
    Ok(())
}

/// `mint approval --env FILE (--mint M ... | --mints-file FILE) [--amm cpmm|clmm|all]`: read-only,
/// no keypair needed.
pub(crate) async fn status(flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    let mints = mints_from(flags)?;
    let amms = amms_from(flags)?;
    // Reading needs no signer; a throwaway key satisfies the chain type.
    let mut chain = RpcChain::new(env.rpc_url.clone(), Keypair::new());
    for mint in &mints {
        let readiness = inspect_readiness(&chain.reader(), *mint)
            .await
            .map_err(|e| approval::redact(&e.to_string()))?;
        let what = match approval::check_mint(&readiness) {
            MintCheck::Approvable(note) if note.is_empty() => "ready to approve".to_string(),
            MintCheck::Approvable(note) => format!("approvable ({note})"),
            MintCheck::NotNeeded(why) => format!("no approval needed ({why})"),
            MintCheck::Blocked(why) => format!("cannot be approved ({why})"),
        };
        println!("{mint}  {what}");
        for amm in &amms {
            let state = approval::state(&mut chain, &env, *amm, mint)
                .await
                .map_err(|e| approval::redact(&e.to_string()))?;
            let text = match state {
                RecordState::Approved => "approved: a pool can be created".to_string(),
                RecordState::NotApproved => {
                    "NOT approved: ask the operator to run `mint approve`".to_string()
                }
                RecordState::Invalid(why) => format!("INVALID record: {why}"),
            };
            println!("  {:<5} {text}", amm.name());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mints_file_allows_blank_lines_and_comments() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        let text = format!("# approved for team 1\n{a}\n\n   {b}   # team 2\n");
        assert_eq!(parse_mints_file(&text).unwrap(), vec![a, b]);
        assert_eq!(parse_mints_file("").unwrap(), vec![]);
    }

    #[test]
    fn a_bad_line_is_named_by_number() {
        let error = parse_mints_file("# header\nnot-a-key\n").unwrap_err();
        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("not-a-key"), "{error}");
    }
}
