//! A readable account of a simulation: did it succeed, how much compute it used, which hook
//! programs ran and how many times, and, if it failed, **whether a hook caused it**.
//!
//! When a hook refuses a Raydium trade the runtime reports a failed transaction, and the useful
//! fact is buried in the logs: Raydium called Token-2022, which called the hook, which failed with a
//! custom code. This pulls that out, so an integrator can tell their user "this token's hook
//! refused the transfer (code 0xb003)", not "the transaction failed".

use solana_sdk::pubkey::Pubkey;

use crate::chain::Simulation;

/// A program that failed with a custom error code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomFailure {
    pub program: Pubkey,
    pub code: u32,
}

/// One hook program's part in a simulation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookRun {
    pub program: Pubkey,
    /// How many times the program was invoked (at any depth).
    pub invocations: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulationReport {
    pub succeeded: bool,
    pub compute_units: Option<u64>,
    /// Each hook program asked about, with how often it ran.
    pub hook_runs: Vec<HookRun>,
    /// The first program to fail with a custom code, if any.
    pub failure: Option<CustomFailure>,
    /// Whether that failure came from one of the hook programs asked about.
    pub hook_refused: bool,
    /// The runtime's own error text, when the simulation failed.
    pub error: Option<String>,
}

/// The first `Program <id> failed: custom program error: 0x<hex>` line in `logs`.
pub fn first_custom_failure(logs: &[String]) -> Option<CustomFailure> {
    logs.iter().find_map(|line| {
        let rest = line.strip_prefix("Program ")?;
        let (program, tail) = rest.split_once(" failed: custom program error: 0x")?;
        let code = u32::from_str_radix(tail.trim(), 16).ok()?;
        Some(CustomFailure {
            program: program.parse().ok()?,
            code,
        })
    })
}

/// Summarise `sim`, asking about `hook_programs` (the hooks on the swap's legs).
pub fn report(sim: &Simulation, hook_programs: &[Pubkey]) -> SimulationReport {
    let failure = first_custom_failure(&sim.logs);
    let hook_refused = failure
        .as_ref()
        .is_some_and(|f| hook_programs.contains(&f.program));
    SimulationReport {
        succeeded: sim.succeeded,
        compute_units: sim.units_consumed,
        hook_runs: hook_programs
            .iter()
            .map(|program| HookRun {
                program: *program,
                invocations: sim.invocations_of(program),
            })
            .collect(),
        failure,
        hook_refused,
        error: sim.error.as_ref().map(|e| e.to_string()),
    }
}

impl SimulationReport {
    /// A few lines for a person.
    pub fn describe(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "simulation {}{}",
            if self.succeeded {
                "succeeded"
            } else {
                "FAILED"
            },
            self.compute_units
                .map(|u| format!(", {u} compute units"))
                .unwrap_or_default()
        ));
        for run in &self.hook_runs {
            lines.push(format!(
                "  hook {} ran {} time(s)",
                run.program, run.invocations
            ));
        }
        if !self.succeeded {
            match (&self.failure, self.hook_refused) {
                (Some(f), true) => lines.push(format!(
                    "  REFUSED BY THE HOOK {} with error code {:#x}: the token's hook rejected this transfer",
                    f.program, f.code
                )),
                (Some(f), false) => lines.push(format!(
                    "  failed in program {} with error code {:#x} (not one of the hooks asked about)",
                    f.program, f.code
                )),
                (None, _) => lines.push(format!(
                    "  failed without a custom program error: {}",
                    self.error.as_deref().unwrap_or("no further detail")
                )),
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(succeeded: bool, logs: Vec<String>) -> Simulation {
        Simulation {
            succeeded,
            error: None,
            logs,
            units_consumed: Some(12_345),
        }
    }

    #[test]
    fn a_hook_failure_is_found_in_the_logs_and_attributed() {
        let raydium = Pubkey::new_unique();
        let token = spl_token_2022::id();
        let hook = Pubkey::new_unique();
        let logs = vec![
            format!("Program {raydium} invoke [1]"),
            format!("Program {token} invoke [2]"),
            format!("Program {hook} invoke [3]"),
            format!("Program {hook} failed: custom program error: 0xb003"),
            format!("Program {token} failed: custom program error: 0xb003"),
            format!("Program {raydium} failed: custom program error: 0xb003"),
        ];
        let report = report(&sim(false, logs), &[hook]);
        let failure = report.failure.clone().unwrap();
        assert_eq!((failure.program, failure.code), (hook, 0xb003));
        assert!(report.hook_refused);
        assert_eq!(report.hook_runs[0].invocations, 1);
        assert!(report.describe().contains("REFUSED BY THE HOOK"));
        assert!(report.describe().contains("0xb003"));
    }

    #[test]
    fn a_failure_elsewhere_is_not_blamed_on_the_hook() {
        let raydium = Pubkey::new_unique();
        let hook = Pubkey::new_unique();
        let logs = vec![
            format!("Program {raydium} invoke [1]"),
            format!("Program {raydium} failed: custom program error: 0x1771"),
        ];
        let report = report(&sim(false, logs), &[hook]);
        assert!(!report.hook_refused);
        assert_eq!(report.failure.unwrap().program, raydium);
    }

    #[test]
    fn a_success_has_no_failure_and_counts_hook_runs() {
        let hook = Pubkey::new_unique();
        let logs = vec![
            format!("Program {hook} invoke [3]"),
            format!("Program {hook} success"),
            format!("Program {hook} invoke [3]"),
            format!("Program {hook} success"),
        ];
        let report = report(&sim(true, logs), &[hook]);
        assert!(report.succeeded && report.failure.is_none() && !report.hook_refused);
        assert_eq!(report.hook_runs[0].invocations, 2);
        assert!(report.describe().contains("ran 2 time(s)"));
    }

    #[test]
    fn non_custom_errors_and_junk_lines_are_ignored() {
        let logs = vec![
            "Program log: custom program error: 0xnothex".to_string(),
            "Program not-a-key failed: custom program error: 0x1".to_string(),
            "Program 11111111111111111111111111111111 failed: insufficient funds".to_string(),
        ];
        assert_eq!(first_custom_failure(&logs), None);
    }
}
