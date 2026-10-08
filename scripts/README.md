# scripts

One script: `deploy.sh`, the "deploy it" step of the repository's purpose (idea -> `rule.rs` ->
test -> **deploy** -> contribute).

```sh
scripts/deploy.sh HOOK_DIR [--cluster devnet|localnet|mainnet-beta|URL] [--keypair FILE] [--limit N]

scripts/deploy.sh starter                          # devnet, ~/.config/solana/id.json
scripts/deploy.sh my-hook --keypair ./deployer.json --limit 1000
```

What it does, in order:

1. Checks `solana`, `cargo` and `cargo build-sbf` are installed and the deployer keypair exists.
2. Picks the cluster and checks the deployer's balance (asks the faucet on devnet or localnet if low).
3. Builds the hook for Solana (`cargo build-sbf`) into `HOOK_DIR/target/deploy`.
4. Deploys it under the program id `cargo build-sbf` generated for it (re-running upgrades the same id).
5. Runs `HOOK_DIR/examples/devnet.rs` if the hook has one: creates a Token-2022 mint with the Transfer
   Hook extension, initialises the hook for that mint, then sends a transfer that must pass and one that
   must be refused.
6. Prints the cluster, hook program id, mint, config and validation-list addresses, and the next step.

Only the [`starter`](../starter) ships the example today; for a template, the script stops after
step 4 and tells you to set the hook up as that template's README describes. Requires Rust and the
[Solana CLI tools](https://solana.com/docs/intro/installation); on Windows run it from Git Bash.

**Safety.** It never defaults to mainnet. The deployer keeps the program's upgrade authority, which
can replace your rule for every mint using it; the script prints how to revoke it. Use a throwaway
devnet keypair unless you know what you are doing.
