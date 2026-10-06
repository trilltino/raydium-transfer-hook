//! Repository automation. Raydium source is never tracked here; `upstream` commands
//! fetch exact locked revisions into the git-ignored `target/upstream/` directory.

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
  upstream list                      print the locked repositories and revisions";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["upstream", rest @ ..] => upstream::run(rest),
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
