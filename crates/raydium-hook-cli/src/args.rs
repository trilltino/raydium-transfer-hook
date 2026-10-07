//! Command-line arguments: a tiny flag parser and keypair loading.

use solana_sdk::signature::{read_keypair_file, Keypair};

pub(crate) const USAGE: &str = "\
raydium-hook <command> [options]

commands:
  deploy   --env FILE --keypair FILE --artifacts DIR --keys DIR [--only NAME]
           Deploy the integration programs (`solana program deploy`), skipping ones already
           deployed, and record each deployment in the environment file.
  e2e      --env FILE --keypair FILE [--fee-receiver-keypair FILE]
           [--amm cpmm|clmm|all] [--hook reference|arbitrary|all] [--record]
           Run the checked end-to-end flows (admin setup, hooked mint, real pool, hooked swaps in
           both directions, hook refusals with rollback). Exits non-zero on any failure.
  inspect  --rpc URL MINT
           Show a mint's Transfer Hook, whether its validation list exists, and who can upgrade
           the hook program.

The deployer keypair is also the admin of the integration builds. Nothing is sent unless the
command says so; secrets are read from the files you name and never printed.";

pub(crate) type Res<T> = Result<T, String>;

pub(crate) struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    pub(crate) positional: Vec<String>,
}

pub(crate) fn parse(args: &[String]) -> Flags {
    let mut flags = Flags {
        values: vec![],
        switches: vec![],
        positional: vec![],
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(name) = arg.strip_prefix("--") {
            if matches!(name, "record") {
                flags.switches.push(name.to_string());
            } else if let Some(value) = args.get(index + 1) {
                flags.values.push((name.to_string(), value.clone()));
                index += 1;
            }
        } else {
            flags.positional.push(arg.clone());
        }
        index += 1;
    }
    flags
}

impl Flags {
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn need(&self, name: &str) -> Res<&str> {
        self.get(name)
            .ok_or_else(|| format!("missing --{name}\n\n{USAGE}"))
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
}

pub(crate) fn keypair(path: &str) -> Res<Keypair> {
    read_keypair_file(path).map_err(|e| format!("cannot read keypair {path}: {e}"))
}
