//! `hook-bench`: run the benchmarks and write `results.json` and `results.md`.
//!
//! ```text
//! cargo xtask localnet build            # the Raydium forks and the hooks
//! cargo build-sbf --manifest-path programs/bench-hook/Cargo.toml --sbf-out-dir target/localnet-sbf
//! cargo run --release -p hook-bench -- [--quick] [--out benches/results]
//! ```

use std::{path::PathBuf, process::Command};

use hook_bench::{artifacts_dir, repo_root, run, Size};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size = if args.iter().any(|a| a == "--quick") {
        Size::Quick
    } else {
        Size::Full
    };
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("benches/results"));

    let artifacts = artifacts_dir();
    if !artifacts.join("bench_hook.so").exists() {
        println!("building the bench hook into {} ...", artifacts.display());
        let status = Command::new("cargo")
            .args(["build-sbf", "--manifest-path"])
            .arg(repo_root().join("programs/bench-hook/Cargo.toml"))
            .arg("--sbf-out-dir")
            .arg(&artifacts)
            .status()
            .expect("could not run `cargo build-sbf`");
        assert!(status.success(), "building the bench hook failed");
    }

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let results = runtime.block_on(run(size));
    std::fs::create_dir_all(&out).expect("create the output directory");
    let (json, markdown) = (out.join("results.json"), out.join("results.md"));
    std::fs::write(&json, results.to_json()).expect("write results.json");
    std::fs::write(&markdown, results.to_markdown()).expect("write results.md");
    println!("\nwrote {} and {}", json.display(), markdown.display());
}
