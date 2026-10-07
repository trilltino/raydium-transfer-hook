//! A quick sweep, to prove the harness runs and that what it measures makes sense. The full sweep
//! behind `benches/results` is `cargo run --release -p hook-bench`.
//!
//! Needs the SBF artifacts: `cargo xtask localnet build` and
//! `cargo build-sbf --manifest-path programs/bench-hook/Cargo.toml --sbf-out-dir target/localnet-sbf`.

use hook_bench::{run, Size};

#[tokio::test]
#[ignore = "needs `cargo xtask localnet build` and the bench hook built into target/localnet-sbf"]
async fn the_quick_sweep_measures_transfers_and_swaps() {
    let results = run(Size::Quick).await;

    // Every transfer row was measured, not skipped. Small hooks succeed outright; a failure is
    // allowed only for the documented reason (Token-2022 running out of its default heap while
    // resolving many extras), and then the larger heap frame must have been tried.
    assert!(!results.transfers.is_empty());
    for row in &results.transfers {
        assert!(
            row.legacy.compute_units.is_some(),
            "N={} legacy had no compute measurement: {:?}",
            row.extras,
            row.legacy.error
        );
        assert!(row.legacy.bytes.is_some());
        match &row.legacy.error {
            None => {}
            Some(error) => {
                assert!(
                    error.contains("out of memory") && row.legacy_with_heap_frame.is_some(),
                    "N={} failed for an undocumented reason: {error}",
                    row.extras
                );
            }
        }
        assert!(
            row.v0_lookup_table
                .error
                .as_deref()
                .map_or(true, |e| e.contains("out of memory")),
            "N={} v0 + lookup table: {:?}",
            row.extras,
            row.v0_lookup_table.error
        );
    }

    // A thicker hook is a bigger transaction: the numbers must move the right way.
    let plain: Vec<_> = results
        .transfers
        .iter()
        .filter(|r| !r.writable_counter)
        .collect();
    let thin = plain.iter().find(|r| r.extras == 0).expect("N = 0");
    let thick = plain.iter().find(|r| r.extras == 16).expect("N = 16");
    assert!(thick.legacy.bytes > thin.legacy.bytes);
    // A lookup table makes the thick hook's transaction much smaller.
    assert!(thick.v0_lookup_table.bytes < thick.legacy.bytes);
    assert!(thick.hook_accounts > thin.hook_accounts);
    assert!(thick.validation_list_bytes > thin.validation_list_bytes);
    assert!(
        thin.legacy.fits_a_packet,
        "a hook with no extras must fit a packet"
    );

    // The swaps ran through real Raydium pools, and at least the thin ones completed.
    assert!(!results.swaps.is_empty());
    assert!(
        results.swaps.iter().any(|s| s.completed),
        "no swap completed: {:?}",
        results.swaps.iter().map(|s| &s.error).collect::<Vec<_>>()
    );
    for swap in results.swaps.iter().filter(|s| s.completed) {
        assert!(swap.compute_units_in.is_some() && swap.compute_units_out.is_some());
    }

    // The report always says what it did not measure.
    let markdown = results.to_markdown();
    assert!(markdown.contains("## Not measured"));
    assert!(markdown.contains("contention under load"));
    assert!(results.to_json().contains("\"not_measured\""));
}
