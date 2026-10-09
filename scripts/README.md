# scripts

One script: `deploy.sh`, the DEPLOY step of UNDERSTAND -> CREATE -> TEST -> **DEPLOY** -> CONTRIBUTE.

```sh
scripts/deploy.sh HOOK_DIR [--cluster devnet|localnet|mainnet-beta|URL] [--keypair FILE] [--allow-mainnet] [-- EXAMPLE_ARGS...]

scripts/deploy.sh starter                          # devnet, ~/.config/solana/id.json
scripts/deploy.sh starter -- --limit 1000          # options after -- go to the hook's example
scripts/deploy.sh templates/fair-launch --keypair ./devnet-deployer.json
scripts/deploy.sh --help
```

The script owns only the common options. Anything after `--` is passed unchanged to
`HOOK_DIR/examples/devnet.rs`, after the `--url`, `--keypair` and `--program-id` the script supplies.

## What it does

**For every hook:**

1. Checks `solana`, `cargo` and `cargo build-sbf` are installed and the deployer keypair exists.
2. Picks the cluster. Mainnet is refused unless you pass `--allow-mainnet`, whether you name it or
   give an RPC URL that serves it (checked by genesis hash).
3. Checks the deployer's balance and asks the faucet on devnet or localnet if it is low.
4. Builds the hook for Solana (`cargo build-sbf`) into `HOOK_DIR/target/deploy`.
5. Deploys it under the program id `cargo build-sbf` generated (re-running upgrades the same id) and
   prints the program id and its upgrade authority.

**Only for hooks that ship `examples/devnet.rs` (today: the [`starter`](../starter) and [`creator-commitment`](../templates/creator-commitment)):**

6. Runs the example: creates a Token-2022 mint whose Transfer Hook points at the program,
   initialises the hook for that mint, sends one transfer that must pass and one the hook must
   refuse, and prints the mint, config and validation-list addresses.

For hooks without the example (today: Fair Launch and Holder Rewards) the script stops after
step 5: the program is deployed but **no mint is created, nothing is initialised and the rule is not
exercised on the cluster**. Each template README has a
`DEPLOY / INITIALIZE` section for the setup, and its tests show the same accounts in-process.

Requires Rust and the [Solana CLI tools](https://solana.com/docs/intro/installation); on Windows run
it from Git Bash.

## Safety

* Devnet is the default. Mainnet needs `--allow-mainnet` on purpose: nothing here is audited.
* The deployer keeps the program's **upgrade authority**, which can replace the rule for every mint
  using it. The script prints how to revoke it (`solana program set-upgrade-authority <ID> --final`).
* Use a throwaway devnet keypair. Never commit keypairs; the program keypair the build generates
  lives in `HOOK_DIR/target/deploy/`.

## Troubleshooting

* `cargo build-sbf` may print `Function ... overflows the maximum allowed frame space` for crypto
  crates inside the dependencies. They are not on paths the hook runs; the build still succeeds.
* The devnet faucet is rate limited. If the airdrop fails, fund the deployer at
  <https://faucet.solana.com> and re-run.
