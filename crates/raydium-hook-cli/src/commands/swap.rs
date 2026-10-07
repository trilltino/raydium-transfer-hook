//! `cpmm swap` and `clmm swap`: trade on the pool a flow kept (`e2e --keep-state`), through the
//! same resolution and framing code the flows use.

use std::collections::HashMap;

use raydium_adapters::{
    clmm::Clmm,
    cpmm::Cpmm,
    swap::{clmm_swap_instruction, cpmm_swap_instruction},
    token::token_amount,
};
use raydium_hook_driver::{chain::Chain, report, resolve_swap_leg, Session};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction, pubkey::Pubkey,
    signature::Signer,
};
use transfer_hook_sdk::{LegHook, LegRole, SplTransferLeg};

use super::{explorer, load_env, rpc_chain};
use crate::args::{Flags, Res};

/// Everything the two AMMs differ in, once the pool is known.
struct Pool {
    mint_0: Pubkey,
    mint_1: Pubkey,
    vault_0: Pubkey,
    vault_1: Pubkey,
    /// Who signs the output transfer.
    authority: Pubkey,
}

/// `cpmm swap` / `clmm swap`
pub(crate) async fn swap(amm: &str, flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    env.require_hook_aware().map_err(|e| e.to_string())?;
    let mut chain = rpc_chain(&env, flags)?;
    let payer = chain.payer().pubkey();
    let session = Session::load(flags.need("state")?).map_err(|e| e.to_string())?;
    if session.amm != amm {
        return Err(format!(
            "that state was kept by a {} flow, not {amm}",
            session.amm
        ));
    }
    let amount = flags.number("amount", 0u64)?;
    if amount == 0 {
        return Err("give --amount N (more than zero)".into());
    }
    let mint0_in = match flags.get("direction").unwrap_or("in") {
        "in" => true,
        "out" => false,
        other => return Err(format!("--direction must be `in` or `out`, not `{other}`")),
    };
    let (mint_0, mint_1) = (
        session.mint_0().map_err(|e| e.to_string())?,
        session.mint_1().map_err(|e| e.to_string())?,
    );
    let accounts = session.accounts().map_err(|e| e.to_string())?;
    let hooks: HashMap<Pubkey, Pubkey> = session
        .hooks()
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();

    // The pool is derived, never looked up: its address follows from the program, the config
    // index and the two mints.
    let (pool, cpmm_pool, clmm_pool, cpmm, clmm);
    match amm {
        "cpmm" => {
            let program = Cpmm {
                program_id: env.cpmm_program().map_err(|e| e.to_string())?,
                fee_receiver: env.cpmm_fee_receiver_key().map_err(|e| e.to_string())?,
            };
            let p = program.pool(program.amm_config(0), mint_0, mint_1);
            pool = Pool {
                mint_0,
                mint_1,
                vault_0: p.vault_0,
                vault_1: p.vault_1,
                authority: p.authority,
            };
            (cpmm, cpmm_pool, clmm, clmm_pool) = (Some(program), Some(p), None, None);
        }
        _ => {
            let program = Clmm {
                program_id: env.clmm_program().map_err(|e| e.to_string())?,
            };
            let p = program.pool(program.amm_config(0), mint_0, mint_1);
            pool = Pool {
                mint_0,
                mint_1,
                vault_0: p.vault_0,
                vault_1: p.vault_1,
                authority: p.pool_state,
            };
            (cpmm, cpmm_pool, clmm, clmm_pool) = (None, None, Some(program), Some(p));
        }
    }
    let pool_account = match (&cpmm_pool, &clmm_pool) {
        (Some(p), _) => p.pool_state,
        (_, Some(p)) => p.pool_state,
        _ => unreachable!(),
    };
    if chain
        .account(&pool_account)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!(
            "the pool {pool_account} does not exist on this cluster: was this state kept against another environment?"
        ));
    }

    let (in_account, out_account) = if mint0_in {
        (accounts[0], accounts[1])
    } else {
        (accounts[1], accounts[0])
    };
    let (in_mint, out_mint, in_vault, out_vault) = if mint0_in {
        (pool.mint_0, pool.mint_1, pool.vault_0, pool.vault_1)
    } else {
        (pool.mint_1, pool.mint_0, pool.vault_1, pool.vault_0)
    };

    // The only writable extras accepted: none, the ones the flow's hooks declared, or the ones
    // named on the command line.
    let writable: Vec<Pubkey> = if flags.has("allow-all-writable") {
        let all = session.allowed_writable().map_err(|e| e.to_string())?;
        println!("accepting every writable extra the hooks declare: {all:?}");
        all
    } else if let Some(list) = flags.get("allow-writable") {
        list.split(',')
            .map(|k| {
                k.trim()
                    .parse()
                    .map_err(|e| format!("--allow-writable: {e}"))
            })
            .collect::<Res<Vec<Pubkey>>>()?
    } else {
        Vec::new()
    };
    let resolve = |role, leg: SplTransferLeg| {
        let expected = hooks.get(&leg.mint).copied();
        let writable = if expected.is_some() {
            writable.clone()
        } else {
            Vec::new()
        };
        (role, leg, expected, writable)
    };
    let expected_out = amount.saturating_sub(2);
    let (r1, l1, e1, w1) = resolve(
        LegRole::Input,
        SplTransferLeg {
            source: in_account,
            mint: in_mint,
            destination: in_vault,
            authority: payer,
            amount,
        },
    );
    let (r2, l2, e2, w2) = resolve(
        LegRole::Output,
        SplTransferLeg {
            source: out_vault,
            mint: out_mint,
            destination: out_account,
            authority: pool.authority,
            amount: expected_out,
        },
    );
    let input: LegHook = resolve_swap_leg(&chain, r1, l1, e1, w1)
        .await
        .map_err(|e| e.to_string())?;
    let output: LegHook = resolve_swap_leg(&chain, r2, l2, e2, w2)
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "{amm} swap {amount} of {in_mint} for {out_mint}\n  input leg hooked: {} ({} accounts)\n  output leg hooked: {} ({} accounts)",
        input.is_hooked(),
        input.account_count(),
        output.is_hooked(),
        output.account_count()
    );

    let min_out = flags.number("min-out", 1u64)?;
    let instruction: Instruction = match amm {
        "cpmm" => cpmm_swap_instruction(
            &cpmm.expect("cpmm"),
            &cpmm_pool.expect("cpmm pool"),
            payer,
            mint0_in,
            in_account,
            out_account,
            amount,
            min_out,
            &input,
            &output,
        ),
        _ => {
            let p = clmm_pool.expect("clmm pool");
            // The tick arrays the flows leave behind: see docs/forking.md.
            let ticks = if mint0_in {
                p.tick_arrays
            } else {
                [p.tick_arrays[1], p.tick_arrays[0]]
            };
            clmm_swap_instruction(
                &clmm.expect("clmm"),
                &p,
                payer,
                mint0_in,
                in_account,
                out_account,
                amount,
                min_out,
                &ticks,
                &input,
                &output,
            )
        }
    }
    .map_err(|e| format!("framing the swap failed: {e:?}"))?;
    let transaction = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
        instruction,
    ];

    let hook_programs: Vec<Pubkey> = hooks.values().copied().collect();
    let sim = chain
        .simulate(&transaction, &[])
        .await
        .map_err(|e| e.to_string())?;
    let summary = report(&sim, &hook_programs);
    println!("\n{}", summary.describe());
    if !summary.succeeded {
        return Err(if summary.hook_refused {
            "the swap was refused by a hook; nothing was sent".into()
        } else {
            "the simulation failed; nothing was sent".into()
        });
    }
    if flags.has("simulate-only") {
        println!("\n--simulate-only: nothing was sent");
        return Ok(());
    }
    let balance = |data: Option<solana_sdk::account::Account>| {
        data.and_then(|a| token_amount(&a.data)).unwrap_or(0)
    };
    let before = (
        balance(
            chain
                .account(&in_account)
                .await
                .map_err(|e| e.to_string())?,
        ),
        balance(
            chain
                .account(&out_account)
                .await
                .map_err(|e| e.to_string())?,
        ),
    );
    let sent = chain
        .send(&transaction, &[])
        .await
        .map_err(|e| format!("the swap failed: {e}"))?;
    let after = (
        balance(
            chain
                .account(&in_account)
                .await
                .map_err(|e| e.to_string())?,
        ),
        balance(
            chain
                .account(&out_account)
                .await
                .map_err(|e| e.to_string())?,
        ),
    );
    println!(
        "\nsent: {}\n  input account  {} -> {}\n  output account {} -> {}",
        explorer(&env, &sent.signature),
        before.0,
        after.0,
        before.1,
        after.1
    );
    Ok(())
}
