//! Command-line arguments: a tiny flag parser and keypair loading.

use std::str::FromStr;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
};

pub(crate) const USAGE: &str = "\
raydium-hook <command> [options]

build, ship and run a hook
  hook build   DIR [--out DIR]
               Build a hook program with `cargo build-sbf`; prints the artifact, its size and hash.
  hook deploy  --env FILE --keypair FILE --so FILE --name NAME [--keys DIR] [--program-keypair FILE]
               Deploy one program (a new program keypair is made under --keys if none exists) and
               record it in the environment file.
  hook setup   --env FILE --keypair FILE --kind KIND --mint MINT [--program ID] [--setup FILE]
               [--pool-vault KEY] [--creator-account KEY] [--reward-mint KEY] [--max-transfer N]
               [--max-per-slot N] [--vest-seconds N] [--window-seconds N] [--reward-seconds N]
               Point the mint at the hook and initialise it. KIND: reference, arbitrary, generic
               (needs --setup FILE), creator-commitment, fair-launch,
               fair-launch-per-slot (the anti-bundle setting), holder-rewards, holder-rewards-one-time
               (a spin-off: funded once).
  hook inspect MINT --rpc URL
               Same as `inspect`.
  mint create  --env FILE --keypair FILE [--decimals N] [--hook PROGRAM | --hookable]
               [--transfer-fee-bps N]
               [--supply N]
               Create a Token-2022 mint (--hook: TransferHook extension pointed at PROGRAM now;
               --hookable: the extension with no hook yet, to attach one after a pool exists),
               an account for the payer, and optionally mint a supply.
  mint approve --env FILE --keypair ADMIN.json (--mint MINT ... | --mints-file FILE)
               [--amm cpmm|clmm|all] [--dry-run]
               Approve hooked mints so a pool can be created with them. CPMM and CLMM accept a
               Token-2022 mint with a TransferHook extension only if the program admin has
               approved it, and only the admin can: the keypair must be the admin the environment
               records. Mints already approved are skipped; each transaction is simulated first;
               --dry-run stops there. --mint can be repeated; the file has one address per line.
  mint approval --env FILE (--mint MINT ... | --mints-file FILE) [--amm cpmm|clmm|all]
               Read-only, no keypair: is each mint approved on each AMM?

run it through Raydium
  e2e          --env FILE --keypair FILE [--fee-receiver-keypair FILE] [--amm cpmm|clmm|all]
               [--hook NAME|all | --hook-dir DIR [--setup FILE] [--keys DIR] | --setup FILE]
               [--second-hook NAME] [--transfer-fee-bps N] [--keep-state FILE]
               [--vest-seconds N] [--window-seconds N] [--reward-seconds N] [--exact-output] [--liquidity] [--record]
               Run the checked end-to-end flows: admin setup, hooked mint, real pool, hooked swaps
               in both directions, the hook's refusals with rollback, its own follow-up steps.
               NAME: reference, arbitrary, creator-commitment, fair-launch,
               fair-launch-per-slot, holder-rewards, holder-rewards-one-time. --hook-dir builds and deploys a hook you wrote
               and sets it up from DIR/setup.json (or --setup FILE); --setup alone runs a deployed
               hook known only by that description. Prints a results table derived from what
               happened and exits non-zero on any failure. --keep-state saves the pool for
               `cpmm swap`. --exact-output also swaps for an exact output amount on CPMM
               (`swap_base_output_v2`; needs a CPMM build that has it). --liquidity also creates a second
               CPMM pool with the hook live and deposits, withdraws and collects fees (`*_v2`).
               --record writes evidence (and a --hook-dir deployment) to the env file.
  ui-fixture   --env FILE --keypair FILE --wallet PUBKEY --out FILE [--amm cpmm|clmm] [--hook NAME] [--fee-receiver-keypair FILE]
               [--window-seconds N] [--max-buy N] [--max-wallet N] [--max-buys-per-slot N] [--max-priority N]
               [--seed-amount N] [--wallet-hooked-amount N] [--wallet-quote-amount N] [--wallet-lamports N]
               Set up a Fair Launch CPMM pool and give WALLET SOL and funded token accounts, then stop:
               what the browser UI and its end-to-end test need. Only the wallet's public key is used.
               The limits are raw token units; the launch window starts now.
  cpmm swap    --env FILE --keypair FILE --state FILE --amount N [--direction in|out]
  clmm swap    --env FILE --keypair FILE --state FILE --amount N [--direction in|out]
               [--min-out N] [--allow-writable KEY,KEY | --allow-all-writable] [--simulate-only]
               Swap on the pool a flow kept: resolve each leg's hook accounts, simulate, say whether
               a hook refused and why, then send. `in` sells mint_0 into the pool.

inspect and check
  inspect      --rpc URL MINT
               A mint's transport readiness: the hook, its program, who can upgrade it, whether the
               validation list is sound. (It says nothing about whether the hook is trustworthy.)
  env probe    --env FILE --keypair FILE
               Does each Raydium program in the environment recognise the hook-aware instructions
               (swap_base_input_v2, swap_v3)? Nothing is sent; it simulates a malformed call.

deploy the whole environment
  deploy       --env FILE --keypair FILE --artifacts DIR --keys DIR [--only NAME]
               Deploy every program the environment lists (skipping ones already deployed) and
               record each deployment with its hash, lockfile hash and toolchain.

The deployer keypair is also the admin of the integration builds. Nothing is sent unless the
command says so; secrets are read from the files you name and never printed.";

pub(crate) type Res<T> = Result<T, String>;

pub(crate) struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    pub(crate) positional: Vec<String>,
}

const SWITCHES: &[&str] = &[
    "record",
    "simulate-only",
    "allow-all-writable",
    "hookable",
    "exact-output",
    "liquidity",
    "dry-run",
];

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
            if SWITCHES.contains(&name) {
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

    /// Every value given for a repeatable flag, in order.
    pub(crate) fn all(&self, name: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// A number flag, or `default` when it is absent.
    pub(crate) fn number<T: FromStr>(&self, name: &str, default: T) -> Res<T>
    where
        T::Err: std::fmt::Display,
    {
        match self.get(name) {
            None => Ok(default),
            Some(text) => text.parse().map_err(|e| format!("--{name}: {e}")),
        }
    }

    /// A required public key flag.
    pub(crate) fn pubkey(&self, name: &str) -> Res<Pubkey> {
        Pubkey::from_str(self.need(name)?).map_err(|e| format!("--{name}: {e}"))
    }

    /// An optional public key flag.
    pub(crate) fn pubkey_opt(&self, name: &str) -> Res<Option<Pubkey>> {
        self.get(name)
            .map(|text| Pubkey::from_str(text).map_err(|e| format!("--{name}: {e}")))
            .transpose()
    }

    /// The first positional argument after the command words.
    pub(crate) fn first_positional(&self, what: &str) -> Res<&str> {
        self.positional
            .first()
            .map(String::as_str)
            .ok_or_else(|| format!("give {what}\n\n{USAGE}"))
    }
}

pub(crate) fn keypair(path: &str) -> Res<Keypair> {
    read_keypair_file(path).map_err(|e| format!("cannot read keypair {path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn values_switches_and_positionals_are_separated() {
        let flags = parse(&args(&[
            "MINT",
            "--rpc",
            "http://x",
            "--simulate-only",
            "--amount",
            "42",
        ]));
        assert_eq!(flags.positional, vec!["MINT"]);
        assert_eq!(flags.get("rpc"), Some("http://x"));
        assert!(flags.has("simulate-only"));
        assert_eq!(flags.number("amount", 0u64), Ok(42));
        assert_eq!(flags.number("missing", 7u64), Ok(7));
    }

    #[test]
    fn bad_numbers_and_keys_name_the_flag() {
        let flags = parse(&args(&["--amount", "lots", "--mint", "not-a-key"]));
        assert!(flags
            .number("amount", 0u64)
            .unwrap_err()
            .contains("--amount"));
        assert!(flags.pubkey("mint").unwrap_err().contains("--mint"));
        assert!(flags.need("nope").unwrap_err().contains("missing --nope"));
        assert_eq!(flags.pubkey_opt("absent"), Ok(None));
    }
}
