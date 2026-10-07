//! Repository automation. Raydium source is never tracked here; `upstream` commands
//! fetch exact locked revisions into the git-ignored `target/upstream/` directory.

mod devnet;
mod devnet_doc;
mod localnet;
mod upstream;

use std::process::ExitCode;

const USAGE: &str = "\
cargo xtask <command>

commands:
  upstream verify [--offline]        verify upstream.lock.toml and the no-vendored-source rule
  upstream fetch [NAME...] [--hook] [--locked]
                                     clone/checkout locked upstream revisions into target/upstream/
                                     (--hook checks out the hook-support revision where locked;
                                      --locked refuses dirty trees and unexpected commits)
  upstream list                      print the locked repositories and revisions
  localnet build                     fetch the locked hook-support forks, build them with their
                                     `localnet` feature and build every hook, into target/localnet-sbf
  localnet validator                 run solana-test-validator with every program preloaded
                                     (ids from environments/localnet.json; no private key needed)
  localnet e2e [--skip-build] [--amm cpmm|clmm|all] [--hook NAME|all]
                                     build, start the validator, run `raydium-hook e2e` for every
                                     hook and for the starter built from source, stop the validator
  env deploy-devnet [--skip-build] [--amm A] [--hook H] [--no-record]
                                     verify locks, build the integration forks and hooks, deploy what
                                     is missing to devnet (existing program ids are never replaced),
                                     run the e2e flows, record evidence, regenerate docs/devnet.md
                                     (needs the integration keys in .keys/)
  devnet-doc [--env FILE] [--out FILE]
                                     render an environment manifest (default environments/devnet.json)
                                     as an evidence page (default docs/devnet.md)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["upstream", rest @ ..] => upstream::run(rest),
        ["devnet-doc", rest @ ..] => devnet_doc::run(rest),
        ["localnet", rest @ ..] => localnet::run(rest),
        ["env", rest @ ..] => devnet::run(rest),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
