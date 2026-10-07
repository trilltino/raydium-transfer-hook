//! # Hook benchmarks
//!
//! Measures what a Transfer Hook costs, as a function of how many extra accounts it needs:
//!
//! * [`transfer`]: a raw Token-2022 transfer through a hook with `N` extras, as a legacy transaction
//!   and as a v0 transaction with an address lookup table. Bytes, accounts, writable accounts,
//!   compute units, and the validation list's size and rent.
//! * [`swap`]: a Raydium CPMM or CLMM swap through the same hook on one mint, or on both, with `N`
//!   extras per hooked leg. Compute units and the largest transaction.
//! * [`report`]: the JSON and Markdown the results are written as.
//!
//! The hook measured is `programs/bench-hook`, which does nothing but the shared checks (and,
//! where stated, one write), so the numbers are the cost of the hook's *thickness*, not of a rule.
//! Everything runs in-process on `solana-program-test` executing the real SBF binaries. That is the
//! real runtime but not a validator and not a cluster under load; [`not_measured`] says what
//! therefore cannot be measured here.

pub mod report;
pub mod swap;
pub mod transfer;

use program_test_flows::{root, setup, Setup};
use report::{Environment, NotMeasured, Results, PACKET_DATA_SIZE};
use sha2::{Digest, Sha256};

/// How much to measure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Size {
    /// A handful of configurations, to prove the harness runs (CI).
    Quick,
    /// The sweep behind `benches/results`.
    Full,
}

impl Size {
    fn transfer_extras(self) -> &'static [u8] {
        match self {
            Size::Quick => &[0, 4, 16],
            Size::Full => &[0, 1, 2, 4, 8, 10, 12, 14, 16, 24, 32, 40, 48, 56, 64],
        }
    }

    fn writable_transfer_extras(self) -> &'static [u8] {
        match self {
            Size::Quick => &[4],
            Size::Full => &[1, 8, 32],
        }
    }

    fn single_leg_extras(self) -> &'static [u8] {
        match self {
            Size::Quick => &[0, 4],
            Size::Full => &[0, 1, 4, 8, 10, 12, 14, 16, 24],
        }
    }

    fn dual_leg_extras(self) -> &'static [u8] {
        match self {
            Size::Quick => &[1],
            Size::Full => &[0, 1, 4, 6, 8, 10, 12],
        }
    }

    fn writable_swap_extras(self) -> &'static [u8] {
        match self {
            Size::Quick => &[],
            Size::Full => &[1, 8],
        }
    }
}

/// What this harness cannot measure, and why. Stated in every report so nobody reads an absence as
/// a result.
pub fn not_measured() -> Vec<NotMeasured> {
    let item = |what: &str, why: &str| NotMeasured {
        what: what.into(),
        why: why.into(),
    };
    vec![
        item(
            "contention under load",
            "solana-program-test runs one transaction at a time. Writable-account serialisation is a \
             property of a cluster's scheduler under concurrent load; the `write` rows show only what \
             the extra write costs in compute, not any waiting.",
        ),
        item(
            "v1 transactions (SIMD-0385)",
            "the pinned solana-sdk 2.2.2 cannot build or sign them, and the repository forbids moving \
             its dependency line opportunistically. Measuring v1 needs a deliberate compatibility \
             migration first.",
        ),
        item(
            "swaps as v0 transactions with a lookup table",
            "the driver's chains send legacy transactions, so swaps are measured as legacy only. The \
             transfer table shows the v0 + lookup-table effect on the hook's own accounts.",
        ),
        item(
            "a real cluster",
            "every number is in-process: no network, no validator, no leader scheduling, so no latency \
             or confirmation time.",
        ),
        item(
            "loaded-accounts data size and CPI trace limits beyond the depth reached",
            "the in-process runtime does not report them per transaction.",
        ),
    ]
}

fn sha256(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn environment(setup: &Setup) -> Environment {
    let toolchain = std::process::Command::new("cargo")
        .args(["build-sbf", "--version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        });
    let artifacts = ["bench_hook.so", "raydium_cp_swap.so", "raydium_clmm.so"]
        .iter()
        .filter_map(|name| sha256(&setup.artifacts.join(name)).map(|sha| (name.to_string(), sha)))
        .collect();
    Environment {
        runtime: "solana-program-test executing the SBF binaries (the real runtime, in-process; \
                  not a validator, not a cluster under load)"
            .into(),
        toolchain,
        artifacts,
        compute_unit_limit: transfer::COMPUTE_UNIT_LIMIT,
        packet_data_size: PACKET_DATA_SIZE,
    }
}

/// The directory the SBF artifacts are read from (and `bench_hook.so` must be in).
pub fn artifacts_dir() -> std::path::PathBuf {
    setup().artifacts
}

/// The repository root.
pub fn repo_root() -> std::path::PathBuf {
    root()
}

/// Run every benchmark of `size`. Progress is printed as it goes.
pub async fn run(size: Size) -> Results {
    let setup = setup();
    let mut results = Results {
        environment: environment(&setup),
        not_measured: not_measured(),
        ..Default::default()
    };
    for extras in size.transfer_extras() {
        println!("transfer, N = {extras}");
        results
            .transfers
            .push(transfer::measure(*extras, false).await);
    }
    for extras in size.writable_transfer_extras() {
        println!("transfer with a writable counter, N = {extras}");
        results
            .transfers
            .push(transfer::measure(*extras, true).await);
    }
    for amm in ["cpmm", "clmm"] {
        for extras in size.single_leg_extras() {
            println!("{amm} swap, one hooked leg, N = {extras}");
            results
                .swaps
                .push(swap::measure(&setup, amm, 1, *extras, false).await);
        }
        for extras in size.dual_leg_extras() {
            println!("{amm} swap, both legs hooked, N = {extras}");
            results
                .swaps
                .push(swap::measure(&setup, amm, 2, *extras, false).await);
        }
        for extras in size.writable_swap_extras() {
            println!("{amm} swap, one hooked leg with a writable counter, N = {extras}");
            results
                .swaps
                .push(swap::measure(&setup, amm, 1, *extras, true).await);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_sweep_goes_past_what_fits_a_legacy_transaction() {
        // The point of the sweep is to find the edge, so it must reach well past it.
        assert!(Size::Full.transfer_extras().iter().any(|n| *n >= 48));
        assert!(Size::Quick.transfer_extras().len() < Size::Full.transfer_extras().len());
    }

    #[test]
    fn what_is_not_measured_is_always_stated() {
        let items = not_measured();
        assert!(items.iter().any(|i| i.what.contains("contention")));
        assert!(items.iter().any(|i| i.what.contains("v1")));
    }
}
