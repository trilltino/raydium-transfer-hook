//! `raydium-hook`: deploy the integration programs, inspect a hooked mint, and run the hooked
//! Raydium swap flows end to end against a real cluster. It is a thin shell over
//! `raydium-hook-driver`, the same code the local tests run.

mod args;
mod commands;

use args::{parse, Res, USAGE};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(run(&args))
    {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("\nerror: {message}");
            1
        }
    };
    std::process::exit(code);
}

async fn run(args: &[String]) -> Res<()> {
    let words: Vec<&str> = args.iter().take(2).map(String::as_str).collect();
    // Two-word commands (`hook build`, `cpmm swap`, ...) consume both words before the flags.
    let (command, rest) = match words.as_slice() {
        [group @ ("hook" | "mint" | "cpmm" | "clmm" | "env"), action, ..] => {
            (format!("{group} {action}"), &args[2..])
        }
        [single, ..] => (single.to_string(), &args[1..]),
        [] => return Err(USAGE.into()),
    };
    let flags = parse(rest);
    match command.as_str() {
        "deploy" => commands::deploy::deploy(&flags).await,
        "e2e" => commands::e2e::e2e(&flags).await,
        "inspect" | "hook inspect" => commands::hook::inspect_hook(&flags).await,
        "hook build" => commands::hook::build(&flags),
        "hook deploy" => commands::hook::deploy(&flags).await,
        "hook setup" => commands::hook::setup(&flags).await,
        "mint create" => commands::mint::create(&flags).await,
        "cpmm swap" => commands::swap::swap("cpmm", &flags).await,
        "clmm swap" => commands::swap::swap("clmm", &flags).await,
        "env probe" => commands::probe::probe(&flags).await,
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
