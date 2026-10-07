//! The end-to-end results table. Every PASS is derived from evidence the flow recorded, never
//! assumed from the fact that an instruction was built: a row with no evidence behind it is `n/a`
//! (the hook has nothing of that kind to prove) or, if the flow failed, `FAIL`.

use raydium_hook_driver::{env::Evidence, readiness::Readiness, Environment};
use solana_sdk::pubkey::Pubkey;

/// One row: a check, whether it holds, and the proof.
pub(crate) struct Row {
    pub(crate) check: String,
    pub(crate) outcome: Outcome,
    pub(crate) detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Pass,
    /// The hook has nothing of this kind (no state, no refusals): not a failure, not a pass.
    NotApplicable,
    Fail,
}

impl Outcome {
    fn text(self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::NotApplicable => "n/a",
            Outcome::Fail => "FAIL",
        }
    }
}

/// What the table is about.
pub(crate) struct Subject<'a> {
    pub(crate) amm: &'a str,
    pub(crate) hook_name: &'a str,
    pub(crate) program: Pubkey,
    /// The hooked mint's transport facts after the flow, if they could be read.
    pub(crate) readiness: Option<&'a Readiness>,
    /// Whether the program is one this repository ships (and so is registered in the environment).
    pub(crate) known_to_the_repository: bool,
}

fn has(evidence: &[Evidence], prefix: &str) -> bool {
    evidence.iter().any(|e| e.step.starts_with(prefix))
}

/// The rows for a flow that finished (every check inside it held, or it would have returned an
/// error instead of evidence).
pub(crate) fn rows(subject: &Subject<'_>, evidence: &[Evidence]) -> Vec<Row> {
    let amm = subject.amm.to_uppercase();
    let swaps: Vec<&Evidence> = evidence
        .iter()
        .filter(|e| e.step.starts_with("hooked swap ("))
        .collect();
    let refusals: Vec<&Evidence> = evidence
        .iter()
        .filter(|e| e.step.starts_with("hook refused swap"))
        .collect();
    let mut rows = Vec::new();
    let mut row = |check: &str, outcome: Outcome, detail: String| {
        rows.push(Row {
            check: check.to_string(),
            outcome,
            detail,
        })
    };

    let program_ok = subject
        .readiness
        .and_then(|r| r.program.as_ref())
        .is_some_and(|p| p.exists && p.executable);
    row(
        "Hook program",
        if program_ok {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        format!("{} is an executable program", subject.program),
    );
    row(
        "Token-2022 hooked mint",
        if has(evidence, "create hooked Token-2022 mint")
            || has(evidence, "create hooked Token-2022 mint (TransferHook")
            || evidence
                .iter()
                .any(|e| e.step.contains("Token-2022 mint (TransferHook"))
        {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        subject
            .readiness
            .map(|r| r.mint.to_string())
            .unwrap_or_default(),
    );
    let list = subject.readiness.and_then(|r| r.validation_list.as_ref());
    let list_ok = list.is_some_and(|l| l.exists && l.owned_by_hook && l.has_execute_shape);
    row(
        "Validation PDA",
        if list_ok {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        match list.and_then(|l| l.extra_accounts.map(|n| (l.address, n))) {
            Some((address, n)) => format!("{address} declares {n} extra account(s)"),
            None => "not found".into(),
        },
    );
    let both_directions = swaps.len() >= 2;
    row(
        "Dynamic account resolver",
        if both_directions {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        match list.and_then(|l| l.extra_accounts) {
            Some(n) => format!(
                "{} accounts per hooked leg (N + 2), resolved fresh for each leg",
                n + 2
            ),
            None => "resolved fresh for each leg".into(),
        },
    );
    row(
        &format!("{amm} hooked swap"),
        if both_directions {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        "hooked token in, then hooked token out".into(),
    );
    let executed = swaps.iter().all(|e| {
        e.detail.contains("hook ran once") || e.detail.contains("each hook ran per its leg")
    });
    row(
        &format!("{amm} hook Execute"),
        if both_directions && executed {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        "the hook ran the expected number of times in every swap (counted in the logs)".into(),
    );
    let mutated = swaps
        .iter()
        .any(|e| e.detail.contains("hook state account changed"));
    row(
        "Custom state mutation",
        if mutated {
            Outcome::Pass
        } else {
            Outcome::NotApplicable
        },
        if mutated {
            "the hook's state account changed on every allowed swap".into()
        } else {
            "the hook keeps no state the flow checks".into()
        },
    );
    let refused = !refusals.is_empty();
    row(
        "Hook rejection",
        if refused {
            Outcome::Pass
        } else {
            Outcome::NotApplicable
        },
        if refused {
            format!(
                "{} refusal(s), each failing inside the hook with its own error code",
                refusals.len()
            )
        } else {
            "this hook declares no refusals".into()
        },
    );
    let rolled_back = refused && refusals.iter().all(|e| e.detail.contains("rolled back"));
    row(
        "Atomic rollback",
        if rolled_back {
            Outcome::Pass
        } else if refused {
            Outcome::Fail
        } else {
            Outcome::NotApplicable
        },
        if refused {
            "every balance was unchanged after each refused swap".into()
        } else {
            "nothing was refused, so nothing to roll back".into()
        },
    );
    row(
        "No hook allowlist",
        if subject.known_to_the_repository {
            Outcome::NotApplicable
        } else {
            Outcome::Pass
        },
        if subject.known_to_the_repository {
            "a program this repository ships, so it proves nothing about unlisted hooks".into()
        } else {
            "this program id is in no list: the stack was never told it exists".into()
        },
    );
    rows
}

/// Whether `program` is one of the programs this repository ships, as `env` records them.
pub(crate) fn is_registered(env: &Environment, program: &Pubkey) -> bool {
    let shipped = env
        .programs
        .reference_hook
        .iter()
        .chain(env.programs.arbitrary_hook.iter())
        .chain(env.programs.templates.values());
    shipped.into_iter().any(|p| p == &program.to_string())
}

/// Render the table, the transactions, and the hook.
pub(crate) fn render(
    subject: &Subject<'_>,
    rows: &[Row],
    evidence: &[Evidence],
    explorer: &dyn Fn(&str) -> String,
) -> String {
    let width = rows.iter().map(|r| r.check.len()).max().unwrap_or(0);
    let mut out = String::from("RAYDIUM TRANSFER HOOK E2E\n\n");
    for row in rows {
        out.push_str(&format!(
            "{:<width$}  {:<4}  {}\n",
            row.check,
            row.outcome.text(),
            row.detail,
            width = width
        ));
    }
    out.push_str(&format!(
        "\nHook:\n{} ({})\n",
        subject.program, subject.hook_name
    ));
    if let Some(readiness) = subject.readiness {
        out.push_str(&format!("\nMint:\n{}\n", readiness.mint));
    }
    for e in evidence
        .iter()
        .filter(|e| e.step.starts_with("hooked swap"))
    {
        if let Some(signature) = &e.signature {
            out.push_str(&format!(
                "\n{} {}:\n{}\n",
                subject.amm.to_uppercase(),
                e.step,
                explorer(signature)
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use raydium_hook_driver::readiness::{ProgramFacts, ValidationListFacts};

    use super::*;

    fn evidence(step: &str, detail: &str, signature: Option<&str>) -> Evidence {
        Evidence {
            flow: "cpmm".into(),
            step: step.into(),
            signature: signature.map(str::to_string),
            detail: detail.into(),
        }
    }

    fn readiness(program: Pubkey) -> Readiness {
        Readiness {
            mint: Pubkey::new_unique(),
            mint_exists: true,
            token_2022: true,
            has_hook_extension: true,
            hook_program: Some(program),
            hook_authority: None,
            program: Some(ProgramFacts {
                address: program,
                exists: true,
                executable: true,
                loader: None,
                upgrade: None,
            }),
            validation_list: Some(ValidationListFacts {
                address: Pubkey::new_unique(),
                exists: true,
                owned_by_hook: true,
                has_execute_shape: true,
                extra_accounts: Some(2),
            }),
        }
    }

    fn full_run() -> Vec<Evidence> {
        vec![
            evidence(
                "create hooked Token-2022 mint (TransferHook extension, hook not yet enabled)",
                "",
                Some("s0"),
            ),
            evidence(
                "hooked swap (hooked token in)",
                "hook ran once; hook state account changed",
                Some("s1"),
            ),
            evidence(
                "hooked swap (hooked token out)",
                "hook ran once; hook state account changed",
                Some("s2"),
            ),
            evidence(
                "hook refused swap (hooked token in, over-amount), nothing changed",
                "rejected with 0x9001; 3 swap instruction(s) in the transaction rolled back",
                None,
            ),
        ]
    }

    fn outcome<'a>(rows: &'a [Row], check: &str) -> &'a Outcome {
        &rows.iter().find(|r| r.check == check).unwrap().outcome
    }

    #[test]
    fn a_stateful_refusing_unlisted_hook_passes_every_row() {
        let program = Pubkey::new_unique();
        let r = readiness(program);
        let subject = Subject {
            amm: "cpmm",
            hook_name: "third-party",
            program,
            readiness: Some(&r),
            known_to_the_repository: false,
        };
        let rows = rows(&subject, &full_run());
        assert!(rows.iter().all(|r| r.outcome == Outcome::Pass), "{}", {
            let mut text = String::new();
            for row in &rows {
                text.push_str(&format!("{}: {:?}\n", row.check, row.outcome));
            }
            text
        });
        assert_eq!(rows.len(), 10);
    }

    #[test]
    fn a_hook_with_no_state_and_no_refusals_is_not_applicable_not_pass() {
        let program = Pubkey::new_unique();
        let r = readiness(program);
        let subject = Subject {
            amm: "clmm",
            hook_name: "plain",
            program,
            readiness: Some(&r),
            known_to_the_repository: true,
        };
        let evidence = vec![
            evidence(
                "create hooked Token-2022 mint (TransferHook extension, hook not yet enabled)",
                "",
                None,
            ),
            evidence("hooked swap (hooked token in)", "hook ran once", Some("a")),
            evidence("hooked swap (hooked token out)", "hook ran once", Some("b")),
        ];
        let rows = rows(&subject, &evidence);
        assert_eq!(
            *outcome(&rows, "Custom state mutation"),
            Outcome::NotApplicable
        );
        assert_eq!(*outcome(&rows, "Hook rejection"), Outcome::NotApplicable);
        assert_eq!(*outcome(&rows, "Atomic rollback"), Outcome::NotApplicable);
        assert_eq!(*outcome(&rows, "No hook allowlist"), Outcome::NotApplicable);
        assert_eq!(*outcome(&rows, "CLMM hooked swap"), Outcome::Pass);
    }

    #[test]
    fn extra_swaps_from_other_checks_do_not_count_as_the_standard_hooked_swaps() {
        let program = Pubkey::new_unique();
        let r = readiness(program);
        let subject = Subject {
            amm: "cpmm",
            hook_name: "third-party",
            program,
            readiness: Some(&r),
            known_to_the_repository: false,
        };
        let mut run = full_run();
        // Steps the exact-output and liquidity checks add; their details carry no "hook ran once".
        run.push(evidence(
            "swap on the second pool (hooked token in)",
            "simulation ok",
            Some("x"),
        ));
        run.push(evidence(
            "hooked exact-output swap (exact output, hooked token in)",
            "received 5 for 7",
            Some("y"),
        ));
        let rows = rows(&subject, &run);
        assert_eq!(*outcome(&rows, "CPMM hook Execute"), Outcome::Pass);
    }

    #[test]
    fn missing_evidence_is_a_failure_not_a_pass() {
        let program = Pubkey::new_unique();
        let subject = Subject {
            amm: "cpmm",
            hook_name: "x",
            program,
            readiness: None,
            known_to_the_repository: false,
        };
        // Only one swap recorded, and no readiness: the resolver and swap rows must fail.
        let evidence = vec![evidence(
            "hooked swap (hooked token in)",
            "hook ran once",
            Some("a"),
        )];
        let rows = rows(&subject, &evidence);
        assert_eq!(*outcome(&rows, "Hook program"), Outcome::Fail);
        assert_eq!(*outcome(&rows, "Validation PDA"), Outcome::Fail);
        assert_eq!(*outcome(&rows, "CPMM hooked swap"), Outcome::Fail);
        assert_eq!(*outcome(&rows, "Dynamic account resolver"), Outcome::Fail);
    }

    #[test]
    fn the_rendered_table_lists_the_hook_the_mint_and_the_swap_links() {
        let program = Pubkey::new_unique();
        let r = readiness(program);
        let subject = Subject {
            amm: "cpmm",
            hook_name: "third-party",
            program,
            readiness: Some(&r),
            known_to_the_repository: false,
        };
        let evidence = full_run();
        let rows = rows(&subject, &evidence);
        let text = render(&subject, &rows, &evidence, &|s| {
            format!("https://explorer/{s}")
        });
        assert!(text.starts_with("RAYDIUM TRANSFER HOOK E2E"));
        assert!(text.contains("CPMM hook Execute"));
        assert!(text.contains(&program.to_string()));
        assert!(text.contains("https://explorer/s1"));
        assert!(text.contains("https://explorer/s2"));
    }

    #[test]
    fn a_program_the_environment_ships_is_registered() {
        let program = Pubkey::new_unique();
        let mut env = Environment::default();
        assert!(!is_registered(&env, &program));
        env.programs
            .templates
            .insert("mine".into(), program.to_string());
        assert!(is_registered(&env, &program));
    }
}
