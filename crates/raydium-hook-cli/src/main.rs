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
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let flags = parse(rest);
    match command.as_str() {
        "deploy" => commands::deploy::deploy(&flags).await,
        "e2e" => commands::e2e::e2e(&flags).await,
        "inspect" => commands::inspect::inspect(&flags).await,
        _ => Err(USAGE.into()),
    }
}
