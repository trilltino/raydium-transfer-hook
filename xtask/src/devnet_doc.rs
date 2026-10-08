//! `cargo xtask devnet-doc`: render an environment manifest (`environments/*.json`) as the
//! evidence page `docs/devnet.md`, so the page always says exactly what the manifest records.
//!
//! The manifest is written by `raydium-hook deploy` and `raydium-hook e2e --record`. Anyone who
//! redeploys under their own program ids gets their own page by running this.

use std::{fs, path::Path};

use serde_json::Value;

const DEFAULT_ENV: &str = "environments/devnet.json";
const DEFAULT_OUT: &str = "docs/devnet.md";

pub fn run(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    generate(args).map_err(Into::into)
}

fn generate(args: &[&str]) -> Result<(), String> {
    let mut env_path = DEFAULT_ENV;
    let mut out_path = DEFAULT_OUT;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match *flag {
            "--env" => env_path = value,
            "--out" => out_path = value,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let text = fs::read_to_string(env_path).map_err(|e| format!("read {env_path}: {e}"))?;
    let env: Value = serde_json::from_str(&text).map_err(|e| format!("parse {env_path}: {e}"))?;
    let page = render(&env, env_path)?;
    if let Some(parent) = Path::new(out_path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(out_path, &page).map_err(|e| format!("write {out_path}: {e}"))?;
    println!("wrote {out_path} ({} lines)", page.lines().count());
    Ok(())
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

struct Links {
    cluster: String,
}

impl Links {
    fn tx(&self, signature: &str) -> String {
        let short = format!(
            "{}…{}",
            &signature[..signature.len().min(8)],
            &signature[signature.len().saturating_sub(6)..]
        );
        format!(
            "[`{short}`](https://solscan.io/tx/{signature}?cluster={})",
            self.cluster
        )
    }

    fn account(&self, key: &str) -> String {
        format!(
            "[`{key}`](https://solscan.io/account/{key}?cluster={})",
            self.cluster
        )
    }
}

/// With thousands separators: 1152864 -> "1,152,864".
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// One end-to-end run: the steps recorded between two `summary` steps of the same flow.
struct Run<'a> {
    flow: &'a str,
    steps: Vec<&'a Value>,
}

impl Run<'_> {
    fn hook(&self) -> String {
        self.steps
            .iter()
            .map(|s| str_of(s, "step"))
            .find_map(|step| {
                let rest = step.strip_prefix("enable ")?;
                Some(
                    rest.strip_suffix(" on the hooked mint")
                        .unwrap_or(rest)
                        .to_string(),
                )
            })
            .unwrap_or_else(|| "?".into())
    }

    fn swaps(&self) -> Vec<&Value> {
        self.steps
            .iter()
            .copied()
            .filter(|s| str_of(s, "step").starts_with("hooked swap"))
            .collect()
    }

    /// The error codes of every refusal, in order.
    fn refusal_codes(&self) -> Vec<String> {
        self.steps
            .iter()
            .filter(|s| str_of(s, "step").starts_with("hook refused"))
            .filter_map(|s| {
                let detail = str_of(s, "detail");
                let at = detail.find("rejected with ")? + "rejected with ".len();
                Some(detail[at..].split_whitespace().next()?.to_string())
            })
            .collect()
    }

    /// Steps the hook asked for after the standard checks: everything recorded after the last
    /// hooked swap or refusal, other than the clock advancing and the summary.
    fn follow_ups(&self) -> usize {
        let last_standard = self
            .steps
            .iter()
            .rposition(|s| {
                let step = str_of(s, "step");
                step.starts_with("hooked swap") || step.starts_with("hook refused")
            })
            .map_or(0, |i| i + 1);
        self.steps[last_standard..]
            .iter()
            .filter(|s| !matches!(str_of(s, "step"), "summary" | "advance cluster time"))
            .count()
    }
}

fn runs(evidence: &[Value]) -> Vec<Run<'_>> {
    let mut out = Vec::new();
    for flow in ["cpmm", "clmm"] {
        let mut current = Vec::new();
        for entry in evidence.iter().filter(|e| str_of(e, "flow") == flow) {
            current.push(entry);
            if str_of(entry, "step") == "summary" {
                out.push(Run {
                    flow,
                    steps: std::mem::take(&mut current),
                });
            }
        }
    }
    out
}

fn compute_units(swap: &Value) -> String {
    let detail = str_of(swap, "detail");
    detail
        .find(" compute units")
        .and_then(|end| detail[..end].rsplit(' ').next())
        .and_then(|n| n.parse::<u64>().ok())
        .map(thousands)
        .unwrap_or_else(|| "?".into())
}

fn render(env: &Value, env_path: &str) -> Result<String, String> {
    let links = Links {
        cluster: str_of(env, "cluster").to_string(),
    };
    let deployments = env
        .get("deployments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let evidence = env
        .get("evidence")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let program_rows: String = deployments
        .iter()
        .map(|d| {
            format!(
                "| {} | {} | {} |\n",
                str_of(d, "name"),
                links.account(str_of(d, "program_id")),
                str_of(d, "source")
            )
        })
        .collect();
    let artifact_rows: String = deployments
        .iter()
        .map(|d| {
            let sha = str_of(d, "artifact_sha256");
            let lock = str_of(d, "lockfile_sha256");
            let lock = if lock.is_empty() {
                "n/a".to_string()
            } else {
                format!("`{}…`", &lock[..lock.len().min(12)])
            };
            format!(
                "| {} | {} | {} | `{}…` | {} | {} |\n",
                str_of(d, "name"),
                links.account(str_of(d, "program_id")),
                thousands(d.get("artifact_bytes").and_then(Value::as_u64).unwrap_or(0)),
                &sha[..sha.len().min(12)],
                lock,
                links.tx(str_of(d, "signature"))
            )
        })
        .collect();
    // Each distinct build toolchain recorded, so the artifact hashes can be reproduced.
    let mut toolchains: Vec<String> = deployments
        .iter()
        .map(|d| str_of(d, "toolchain").to_string())
        .filter(|t| !t.is_empty())
        .collect();
    toolchains.sort();
    toolchains.dedup();
    let toolchain_note = if toolchains.is_empty() {
        "No build toolchain was recorded for these deployments (they predate the field)."
            .to_string()
    } else {
        format!(
            "Built with: {}.",
            toolchains
                .iter()
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join("; ")
        )
    };

    let all_runs = runs(&evidence);
    let run_rows: String = all_runs
        .iter()
        .filter_map(|run| {
            let swaps = run.swaps();
            let (first, second) = (swaps.first()?, swaps.get(1)?);
            let codes = run.refusal_codes();
            let refusals = if codes.is_empty() {
                "none (this hook never refuses)".to_string()
            } else {
                codes
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            Some(format!(
                "| {} | {} | {} ({} CU) | {} ({} CU) | {} | {} |\n",
                run.flow.to_uppercase(),
                run.hook(),
                links.tx(str_of(first, "signature")),
                compute_units(first),
                links.tx(str_of(second, "signature")),
                compute_units(second),
                refusals,
                run.follow_ups()
            ))
        })
        .collect();
    let pools: String = all_runs
        .iter()
        .filter_map(|run| {
            let summary = run.steps.iter().find(|s| str_of(s, "step") == "summary")?;
            Some(format!(
                "- {} / {}: {}\n",
                run.flow.to_uppercase(),
                run.hook(),
                str_of(summary, "detail")
            ))
        })
        .collect();

    let admin = links.account(str_of(env, "admin"));
    let fee_receiver = links.account(str_of(env, "cpmm_fee_receiver"));
    let runs_count = all_runs.len();
    let name = str_of(env, "name");

    Ok(format!(
        "\
# Devnet evidence

<!-- Generated by `cargo xtask devnet-doc` from `{env_path}`. Do not edit by hand. -->

Official Raydium programs, including Raydium's own devnet deployments, do not contain the hook-aware
instructions (`swap_base_input_v2`, `swap_v3`). To run a hooked swap on a real cluster before
Raydium adopts them, the hook-support forks are built with an `integration` feature and deployed
under **our own program ids**. This page records exactly what is deployed and what ran in the
environment `{name}`. It is a test environment, not an official Raydium deployment, and not an
audit. To get this page for your own deployment, see [forking.md](forking.md).

## Programs

| Program | Id | Built from |
|---|---|---|
{program_rows}
Admin of the Raydium builds and upgrade authority of every program above: {admin}.
CPMM pool-creation fee receiver (a wrapped-SOL token account): {fee_receiver}.
Machine-readable copy, with every transaction: [`{env_path}`](../{env_path}).

## Deployed artifacts

| Name | Program | Bytes | SHA-256 | Cargo.lock SHA-256 | Deploy transaction |
|---|---|---|---|---|---|
{artifact_rows}
{toolchain_note} A hash that does not match after a rebuild usually means a different toolchain or lockfile.

Rent is a refundable deposit held by each program-data account, about 5.1 SOL per MB of program
(a 140 KB hook is about 0.7 SOL, the 1.15 MB CLMM about 5.9). Closing a program returns it to the
upgrade authority.

## What ran

`raydium-hook e2e --env {env_path} --record` ran {runs_count} flows. Each performs real admin setup,
creates a hooked Token-2022 mint and a plain quote mint and a real pool (for CLMM, with a
liquidity position that creates the tick arrays), enables the hook, then:

1. a hooked swap with the hooked token as input, then as output: the hook must run exactly once
   (and a stateful hook's account must change);
2. every swap the hook must refuse: it must fail inside the hook program with the hook's own error
   code, and every balance must be unchanged afterwards;
3. the hook's own follow-up steps, if it has any (wait for a window to end, fund and claim a
   reward, then check the result).

| AMM | Hook | Hooked token in | Hooked token out | Refusals (error codes) | Follow-up steps |
|---|---|---|---|---|---|
{run_rows}
Compute units are from the simulation of each swap, for the whole transaction (Raydium, Token-2022
and the hook). Every flow listed finished with all its checks passing: a flow that fails a check
returns an error and is not recorded.

Pools created in these runs:

{pools}
To check one by hand: `solana confirm -v <signature> --url devnet` shows the call chain Raydium
program, then Token-2022 `TransferChecked`, then the hook, all succeeding.

## Limits of this evidence

- A test environment under our own program ids. It says nothing about official Raydium, whose
  programs reject these instructions.
- Single runs on public devnet; no load, no timing, no address-lookup-table measurements.
- The deployer is the upgrade authority of every program here, so the code can be replaced at any
  time. That is a test-environment choice, not a recommendation.
- Liquidity deposits and withdrawals are not covered: those paths reject hooked mints.
"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separates_groups_of_three() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_152_864), "1,152,864");
    }

    #[test]
    fn a_manifest_renders_with_its_runs() {
        let env: Value = serde_json::json!({
            "name": "t", "cluster": "devnet", "admin": "A", "cpmm_fee_receiver": "F",
            "deployments": [{
                "name": "h", "program_id": "P", "signature": "SIGSIGSIGSIG",
                "artifact_sha256": "abcdef0123456789", "artifact_bytes": 1234,
                "source": "src"
            }],
            "evidence": [
                {"flow": "cpmm", "step": "enable my-hook on the hooked mint", "detail": ""},
                {"flow": "cpmm", "step": "hooked swap (hooked token in)", "signature": "S1",
                 "detail": "simulation ok: 18 log lines, 85918 compute units"},
                {"flow": "cpmm", "step": "hooked swap (hooked token out)", "signature": "S2",
                 "detail": "simulation ok: 18 log lines, 85787 compute units"},
                {"flow": "cpmm", "step": "hook refused swap (hooked token in, x), nothing changed",
                 "detail": "hook P rejected with 0xa005 after 1 hook run(s)"},
                {"flow": "cpmm", "step": "summary", "detail": "program X pool Y"}
            ]
        });
        let page = render(&env, "environments/t.json").unwrap();
        assert!(page.contains("| CPMM | my-hook |"));
        assert!(page.contains("(85,918 CU)"));
        assert!(page.contains("`0xa005`"));
        assert!(page.contains("1,234"));
    }
}
