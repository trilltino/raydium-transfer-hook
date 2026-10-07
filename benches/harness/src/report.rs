//! What the benchmarks record, and how it is written down: machine-readable JSON for tools and a
//! Markdown page for people. Every figure is a measurement on the runtime named in
//! [`Environment`], never a protocol constant, and a configuration that fails is recorded as a
//! failure with its reason, not dropped.

use serde::{Deserialize, Serialize};

/// The heap frame requested when a transfer runs out of the default 32 KiB heap.
pub const HEAP_FRAME_BYTES: u32 = 256 * 1024;

/// The most a legacy transaction can serialise to.
pub const PACKET_DATA_SIZE: usize = 1232;

/// What the numbers were measured on, so they can be reproduced or distrusted.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Environment {
    /// How the programs were run: `solana-program-test` executing the SBF binaries.
    pub runtime: String,
    /// `cargo build-sbf --version`, if available.
    pub toolchain: Option<String>,
    /// SHA-256 of each artifact measured, by file name.
    pub artifacts: Vec<(String, String)>,
    /// The compute-unit limit the harness requested (it is not what a wallet would pay for).
    pub compute_unit_limit: u32,
    pub packet_data_size: usize,
}

/// One transaction format's result for one configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormatResult {
    /// The transaction fits a packet (`<= 1232` bytes). Compute is measured either way.
    pub fits_a_packet: bool,
    pub bytes: Option<usize>,
    /// Account keys in the message (plus those loaded from a lookup table, if any).
    pub accounts: Option<usize>,
    pub writable_accounts: Option<usize>,
    pub compute_units: Option<u64>,
    /// Why the configuration could not be measured or run, if so.
    pub error: Option<String>,
}

/// A raw Token-2022 transfer through a hook with `extras` extra accounts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferRow {
    pub extras: u8,
    pub writable_counter: bool,
    /// Accounts the hook adds to the transfer: `extras + 2`.
    pub hook_accounts: usize,
    pub validation_list_bytes: usize,
    pub validation_list_rent_lamports: u64,
    pub legacy: FormatResult,
    /// The same legacy transaction with a `request_heap_frame` of [`HEAP_FRAME_BYTES`], measured
    /// only when the default heap was not enough (Token-2022 allocates while resolving extras).
    #[serde(default)]
    pub legacy_with_heap_frame: Option<FormatResult>,
    /// A v0 transaction with every non-signer account in an address lookup table.
    pub v0_lookup_table: FormatResult,
    /// Entries in that lookup table.
    pub lookup_table_entries: Option<usize>,
}

/// A Raydium swap through a hook with `extras` extra accounts on each hooked leg.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwapRow {
    pub amm: String,
    /// 1: one hooked leg. 2: both mints hooked (the same program on each).
    pub hooked_legs: u8,
    pub extras: u8,
    pub writable_counter: bool,
    /// Compute units of the whole swap transaction (Raydium + Token-2022 + the hook(s)), hooked
    /// token in, then out.
    pub compute_units_in: Option<u64>,
    pub compute_units_out: Option<u64>,
    /// The largest legacy transaction the flow sent, in bytes.
    pub largest_transaction_bytes: Option<usize>,
    /// The flow completed: every swap landed and every check held.
    pub completed: bool,
    pub error: Option<String>,
}

/// What was not measured, and why.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotMeasured {
    pub what: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Results {
    pub environment: Environment,
    pub transfers: Vec<TransferRow>,
    pub swaps: Vec<SwapRow>,
    pub not_measured: Vec<NotMeasured>,
}

fn cell<T: ToString>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "n/a".into())
}

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

fn units(value: &Option<u64>) -> String {
    value.map(thousands).unwrap_or_else(|| "n/a".into())
}

/// The program that failed first in a transaction's logs: the innermost one, since a failure
/// propagates outwards. Works on both the plain log lines and the debug-printed list the flows
/// put in their errors.
fn first_failed_program(error: &str) -> Option<&str> {
    let mut rest = error;
    while let Some(at) = rest.find("Program ") {
        rest = &rest[at + "Program ".len()..];
        if let Some(end) = rest.find(' ') {
            let (id, after) = rest.split_at(end);
            if after.starts_with(" failed") && id.len() >= 32 {
                return Some(id);
            }
        }
    }
    None
}

/// A short, stable description of why a measurement failed, so identical failures group.
fn cause(error: &str) -> String {
    if error.contains("over the") && error.contains("packet") {
        return "over the packet limit".to_string();
    }
    let memory = error.contains("out of memory") || error.contains("memory allocation failed");
    let who: Option<String> = match first_failed_program(error) {
        Some(id) if id.starts_with("Tokenz") => Some("Token-2022".to_string()),
        Some(id) if id.starts_with("11111") || id.starts_with("Compute") => None,
        Some(id) => Some(format!("program {}…", &id[..6])),
        None => None,
    };
    match (who, memory) {
        (Some(who), true) => format!("{who} ran out of heap (`memory allocation failed`)"),
        (Some(who), false) if error.contains("panicked") => {
            format!("{who} panicked (the log shows no memory message; most likely the same 32 KiB heap)")
        }
        _ => {
            let one_line = error.split_whitespace().collect::<Vec<_>>().join(" ");
            one_line.chars().take(240).collect()
        }
    }
}

/// Compute units of a measurement that ran; "fails" when the transaction itself failed (the units
/// consumed before a failure are not a measurement of anything).
fn cu_cell(result: &FormatResult) -> String {
    match &result.error {
        Some(error) if cause(error) != "over the packet limit" => "fails".to_string(),
        _ => units(&result.compute_units),
    }
}

/// Group `(label, cause)` pairs by cause, keeping first-seen order.
fn group(items: Vec<(String, String)>) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for (label, cause) in items {
        match groups.iter_mut().find(|(c, _)| *c == cause) {
            Some((_, labels)) => labels.push(label),
            None => groups.push((cause, vec![label])),
        }
    }
    groups
}

fn fit(result: &FormatResult) -> &'static str {
    match (result.bytes, result.fits_a_packet) {
        (None, _) => "n/a",
        (Some(_), true) => "yes",
        (Some(_), false) => "NO",
    }
}

impl Results {
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("results serialise");
        text.push('\n');
        text
    }

    pub fn to_markdown(&self) -> String {
        let e = &self.environment;
        let mut out = String::new();
        out.push_str("# Hook benchmark results\n\n");
        out.push_str(
            "<!-- Generated by `cargo run -p hook-bench`; do not edit by hand. -->\n\n\
             Every number is a measurement, on the runtime below, of a hook that does nothing but the\n\
             shared checks (and, where stated, one write). They are not protocol constants and not a\n\
             ranking of hooks: a real rule adds its own compute. A configuration that fails is shown\n\
             as a failure with its reason.\n\n",
        );
        out.push_str(&format!(
            "* runtime: {}\n* compute-unit limit requested: {}\n* packet limit: {} bytes\n",
            e.runtime,
            thousands(e.compute_unit_limit as u64),
            e.packet_data_size
        ));
        if let Some(toolchain) = &e.toolchain {
            out.push_str(&format!("* toolchain: `{toolchain}`\n"));
        }
        for (name, sha) in &e.artifacts {
            out.push_str(&format!(
                "* `{name}` sha256 `{}…`\n",
                &sha[..sha.len().min(12)]
            ));
        }

        out.push_str("\n## A Token-2022 transfer through a hook with N extra accounts\n\n");
        out.push_str(
            "A hook with N extras adds N + 2 accounts (the extras, the hook program, the validation\n\
             list). \"Legacy\" is a legacy transaction; \"v0 + ALT\" is a v0 transaction with every\n\
             non-signer account in an address lookup table.\n\n\
             | N | write | hook accounts | list bytes | list rent (lamports) | legacy bytes | fits | legacy CU | v0+ALT bytes | fits | v0+ALT CU | ALT entries |\n\
             |---|---|---|---|---|---|---|---|---|---|---|---|\n",
        );
        for row in &self.transfers {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                row.extras,
                if row.writable_counter { "yes" } else { "no" },
                row.hook_accounts,
                row.validation_list_bytes,
                thousands(row.validation_list_rent_lamports),
                cell(&row.legacy.bytes),
                fit(&row.legacy),
                cu_cell(&row.legacy),
                cell(&row.v0_lookup_table.bytes),
                fit(&row.v0_lookup_table),
                cu_cell(&row.v0_lookup_table),
                cell(&row.lookup_table_entries),
            ));
        }
        let mut transfer_failures = Vec::new();
        for row in &self.transfers {
            let label = format!(
                "N = {}{}",
                row.extras,
                if row.writable_counter { " (write)" } else { "" }
            );
            for (format, result) in [("legacy", &row.legacy), ("v0 + ALT", &row.v0_lookup_table)] {
                if let Some(error) = &result.error {
                    transfer_failures.push((format!("{label}, {format}"), cause(error)));
                }
            }
        }
        if !transfer_failures.is_empty() {
            out.push_str("\nWhy some cells are \"fails\" or \"NO\":\n\n");
            for (why, labels) in group(transfer_failures) {
                out.push_str(&format!("* {why}: {}\n", labels.join("; ")));
            }
        }
        let heap_retries: Vec<(String, String)> = self
            .transfers
            .iter()
            .filter_map(|row| {
                let retry = row.legacy_with_heap_frame.as_ref()?;
                let label = format!(
                    "N = {}{}",
                    row.extras,
                    if row.writable_counter { " (write)" } else { "" }
                );
                let outcome = match (&retry.error, retry.compute_units) {
                    (None, Some(units)) => {
                        format!(
                            "succeeds ({units} compute units, {} bytes)",
                            retry.bytes.unwrap_or(0)
                        )
                    }
                    (Some(error), _) => format!("still fails: {}", cause(error)),
                    _ => "no measurement".to_string(),
                };
                Some((label, outcome))
            })
            .collect();
        if !heap_retries.is_empty() {
            out.push_str(&format!(
                "\nRe-run with `request_heap_frame({HEAP_FRAME_BYTES})` added to the transaction:\n\n"
            ));
            for (outcome, labels) in group(heap_retries) {
                out.push_str(&format!("* {outcome}: {}\n", labels.join("; ")));
            }
        }

        out.push_str("\n## A Raydium swap through a hook with N extra accounts per hooked leg\n\n");
        out.push_str(
            "Legacy transactions (the driver does not send v0 swaps). The compute is the whole swap:\n\
             Raydium, Token-2022 and every hook that ran. \"Completed\" means every swap landed and\n\
             every check in the flow held.\n\n\
             | AMM | hooked legs | N | write | CU in | CU out | largest tx (bytes) | completed |\n\
             |---|---|---|---|---|---|---|---|\n",
        );
        for row in &self.swaps {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                row.amm.to_uppercase(),
                row.hooked_legs,
                row.extras,
                if row.writable_counter { "yes" } else { "no" },
                units(&row.compute_units_in),
                units(&row.compute_units_out),
                cell(&row.largest_transaction_bytes),
                if row.completed { "yes" } else { "NO" },
            ));
        }
        let swap_failures: Vec<(String, String)> = self
            .swaps
            .iter()
            .filter_map(|row| {
                let error = row.error.as_deref()?;
                Some((
                    format!(
                        "{} with {} hooked leg(s), N = {}",
                        row.amm.to_uppercase(),
                        row.hooked_legs,
                        row.extras
                    ),
                    cause(error),
                ))
            })
            .collect();
        if !swap_failures.is_empty() {
            out.push_str("\nWhy a swap did not complete:\n\n");
            for (why, labels) in group(swap_failures) {
                out.push_str(&format!("* {why}: {}\n", labels.join("; ")));
            }
        }

        out.push_str("\n## Not measured\n\n");
        for item in &self.not_measured {
            out.push_str(&format!("* **{}**: {}\n", item.what, item.why));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Results {
        Results {
            environment: Environment {
                runtime: "solana-program-test (SBF)".into(),
                toolchain: Some("tool 1".into()),
                artifacts: vec![("bench_hook.so".into(), "ab".repeat(32))],
                compute_unit_limit: 1_400_000,
                packet_data_size: PACKET_DATA_SIZE,
            },
            transfers: vec![
                TransferRow {
                    extras: 4,
                    hook_accounts: 6,
                    validation_list_bytes: 160,
                    validation_list_rent_lamports: 2_000_000,
                    legacy: FormatResult {
                        fits_a_packet: true,
                        bytes: Some(900),
                        accounts: Some(14),
                        writable_accounts: Some(3),
                        compute_units: Some(15_200),
                        error: None,
                    },
                    v0_lookup_table: FormatResult {
                        fits_a_packet: true,
                        bytes: Some(500),
                        compute_units: Some(15_300),
                        ..Default::default()
                    },
                    lookup_table_entries: Some(12),
                    ..Default::default()
                },
                TransferRow {
                    extras: 60,
                    hook_accounts: 62,
                    legacy: FormatResult {
                        fits_a_packet: false,
                        bytes: Some(2_600),
                        error: Some("too big".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            swaps: vec![SwapRow {
                amm: "cpmm".into(),
                hooked_legs: 2,
                extras: 8,
                compute_units_in: Some(130_000),
                compute_units_out: Some(131_000),
                largest_transaction_bytes: Some(1_100),
                completed: true,
                ..Default::default()
            }],
            not_measured: vec![NotMeasured {
                what: "contention".into(),
                why: "needs a cluster".into(),
            }],
        }
    }

    #[test]
    fn json_round_trips() {
        let results = sample();
        let back: Results = serde_json::from_str(&results.to_json()).unwrap();
        assert_eq!(back, results);
    }

    #[test]
    fn markdown_shows_numbers_failures_and_what_was_not_measured() {
        let text = sample().to_markdown();
        assert!(text.contains("| 4 | no | 6 | 160 |"));
        assert!(text.contains("15,200"));
        assert!(text.contains("| 130,000 | 131,000 | 1100 | yes |"));
        // A configuration that did not fit says so and says why.
        assert!(text.contains("| NO |") || text.contains("NO"));
        assert!(text.contains("too big: N = 60, legacy"));
        assert!(text.contains("**contention**: needs a cluster"));
        assert!(text.contains("`bench_hook.so` sha256 `abababababab…`"));
    }

    #[test]
    fn failures_are_grouped_by_cause_and_failed_rows_show_no_compute() {
        assert_eq!(
            cause("Program TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb failed: Program log: Error: memory allocation failed, out of memory"),
            "Token-2022 ran out of heap (`memory allocation failed`)"
        );
        assert_eq!(
            cause("the transaction is 1460 bytes, over the 1232 byte packet limit"),
            "over the packet limit"
        );
        let failed = FormatResult {
            compute_units: Some(98_000),
            error: Some("Program failed: out of memory".into()),
            ..Default::default()
        };
        assert_eq!(cu_cell(&failed), "fails");
        let too_big = FormatResult {
            compute_units: Some(98_000),
            error: Some("1441 bytes is over the 1232-byte packet".into()),
            ..Default::default()
        };
        assert_eq!(cu_cell(&too_big), "98,000");
        let grouped = group(vec![
            ("N = 16".into(), "a".into()),
            ("N = 24".into(), "a".into()),
            ("N = 32".into(), "b".into()),
        ]);
        assert_eq!(
            grouped,
            vec![
                (
                    "a".to_string(),
                    vec!["N = 16".to_string(), "N = 24".to_string()]
                ),
                ("b".to_string(), vec!["N = 32".to_string()]),
            ]
        );
    }

    #[test]
    fn a_missing_measurement_is_na_not_zero() {
        assert_eq!(cell::<u64>(&None), "n/a");
        assert_eq!(units(&None), "n/a");
        assert_eq!(fit(&FormatResult::default()), "n/a");
    }
}
