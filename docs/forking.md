# Forking this repository

What you can do with a fork, what you can reuse unchanged, and what you must change. Be clear about
which of the three goals you have, because the cost differs a lot.

| Goal | You need | Effort |
|---|---|---|
| **A. Write and test a hook** | a Rust toolchain | minutes: `cargo test` |
| **B. Run your hook through real Raydium pools, in-process** | the two hook-aware Raydium binaries built with *your* keys | an afternoon |
| **C. Do B on a real cluster and publish the evidence** | B, plus devnet SOL (about 13 SOL of refundable rent for everything, much less for just your own hook) | a day |

## A. Write and test a hook (no keys, no Raydium, no network)

```sh
git clone <your fork>
cd raydium-transfer-hook
cp -r templates/transfer-hook-starter my-hook    # or an example; PowerShell: Copy-Item -Recurse
cd my-hook
cargo test                                       # native
cargo build-sbf                                  # the deployable program
```

To run it through real Raydium pools on a local validator, still with no keys:
`cargo xtask localnet build`, `cargo xtask localnet validator`, then
`raydium-hook e2e --env environments/localnet.json --keypair tests/fixtures/localnet/admin.json
--keys target/localnet/keys --amm all --hook-dir my-hook` (see the README).

Your rule is `src/rule.rs`. The tests run a real Token-2022 transfer through your hook in-process
and check exact error codes and rollback. Nothing else in this repository is needed for this. See
[authoring-hooks.md](authoring-hooks.md).

To use the shared plumbing and the in-process test world (`hook-kit`), put your hook under
`templates/` as a workspace member, as the five examples are.

## B and C. Run it through Raydium

### What needs no keys at all

`cargo xtask localnet build` builds the two forks with their `localnet` feature: the **upstream
program ids** and an admin read at build time from `environments/localnet.json`, whose key is the
throwaway one committed in `tests/fixtures/localnet`. The CPMM fee receiver is seeded at genesis. So
a fork runs every Raydium flow, in-process and on `solana-test-validator`, with nothing to set up:

```sh
cargo xtask localnet build
cargo test -p program-test-flows -p third-party-hook-acceptance -- --ignored
cargo xtask localnet e2e --skip-build
```

### What a public cluster needs

For devnet the forks are built with their `integration` feature instead, which **bakes in a program
id, an admin key and a fee-receiver key** (the localnet ids are upstream's mainnet ids, which nobody
can deploy to). The committed `environments/devnet.json` uses *our* ids, and the keys behind them
are git-ignored and never published. So a fork deploying to a public cluster needs its own.

### Steps

1. **Fork the two Raydium forks** ([`trilltino/raydium-cp-swap`](https://github.com/trilltino/raydium-cp-swap)
   and [`trilltino/raydium-clmm`](https://github.com/trilltino/raydium-clmm), branch
   `transfer-hook-support`) and set your own program id, admin and fee-receiver in each
   `integration` feature (search the source for `feature = "integration"`). Update
   `upstream.lock.toml` to your commits, then check it: `cargo xtask upstream verify`.
2. **Make your keys** in `.keys/` (git-ignored; never commit them). `solana-keygen new` for each:

   | File | Used for |
   |---|---|
   | `deployer.json` | pays, and is the admin baked into the Raydium builds |
   | `cpmm-program.json`, `clmm-program.json` | the two Raydium program ids |
   | `cpmm-fee-receiver.json` | the CPMM pool-creation fee receiver (a wrapped-SOL token account) |
   | `hook-program.json`, `arbitrary-hook-program.json` | the reference and arbitrary hooks |
   | `creator-commitment-program.json`, `fair-launch-program.json`, `anti-bundle-program.json`, `loyalty-rewards-program.json`, `parent-spin-off-program.json` | the five examples |

3. **Point an environment at your ids.** Copy `environments/devnet.json` to, say,
   `environments/mine.json`; replace `programs`, `admin` and `cpmm_fee_receiver`, and empty
   `deployments` and `evidence`.
4. **Build, deploy, run and record** in one command:

   ```sh
   cargo xtask env deploy-devnet --env environments/mine.json
   ```

   It verifies the locks, builds the forks (`integration`) and every hook into
   `target/integration-sbf`, deploys whatever is missing (`raydium-hook deploy` skips programs that
   already exist, so recorded ids are never replaced, and records each deployment with its SHA-256,
   lockfile hash and toolchain), runs `raydium-hook e2e --record` for every hook through both AMMs,
   and regenerates the evidence page. `--hook NAME` and `--amm cpmm|clmm` narrow the run. Hooks
   with a time window really wait on a live cluster; the individual commands take
   `--vest-seconds`, `--window-seconds` and `--reward-seconds`.

   The same steps by hand: `raydium-hook deploy --env FILE --keypair .keys/deployer.json
   --artifacts target/integration-sbf --keys .keys`, then `raydium-hook e2e --env FILE --keypair
   .keys/deployer.json --fee-receiver-keypair .keys/cpmm-fee-receiver.json --amm all --hook all
   --record`, then `cargo xtask devnet-doc --env FILE --out docs/devnet.md`.

5. **Run the in-process flows against these exact binaries** (optional):
   `RTH_PROFILE=integration cargo test -p program-test-flows -- --ignored`.

## Adding your hook to the flows

Nothing has to change for a hook to run: `raydium-hook e2e --hook-dir DIR` builds, deploys and
sets up a hook from its `setup.json`, and `--setup FILE` runs an already-deployed one known only
by that description (see `crates/raydium-hook-driver/src/hooks/generic.rs`). That is the
permissionless path, and the one to use for your own hook.

To make a hook one of the repository's named examples (with follow-up steps a JSON description
cannot express, like advancing time or claiming), add a provider:

1. `crates/raydium-hook-driver/src/hooks/<yours>.rs`: implement `HookSetup` (how to point a mint at
   your hook and initialise your state, which swaps it must refuse and with which error code, which
   writable extras the integrator accepts, and any follow-up steps). Export it from
   `hooks/mod.rs` and `lib.rs`. The five examples are good models.
2. Add your program id under `programs.templates` in your environment manifests
   (`environments/localnet.json` too, with a throwaway keypair in `tests/fixtures/localnet`).
3. Add it to `ARTIFACTS` in `crates/raydium-hook-cli/src/commands/deploy.rs`, to `program_for` and
   `provider` in `commands/hook.rs`, to `SHIPPED` in `commands/e2e.rs`, and to `HOOKS` in
   `xtask/src/localnet.rs`.
4. Add a test next to the others in `tests/program-test/tests/local_flows.rs`.

## Do not change

* **The pinned dependency line** (`solana-program 2.2.1`, `solana-program-test 2.2.7`,
  `solana-sdk 2.2.2`, `spl-token 7.0.0`, `spl-token-2022 7.0.0`, `spl-transfer-hook-interface 0.10.0`,
  `spl-tlv-account-resolution 0.10.0`). It is exact on purpose; see
  [source-lock.md](source-lock.md).
* **Per-leg resolution.** The two transfers of a swap get independent slices that are never merged,
  deduplicated or reordered. Everything else rests on that.
* **No Raydium source in the repository.** `cargo xtask upstream verify` fails if tracked paths look
  like copied Raydium source; keep it that way and keep the pins in `upstream.lock.toml`.

## Windows note

The in-process runtime pulls in a vendored OpenSSL build that needs a complete Perl. If a fresh
build directory fails in `openssl-sys`, reuse an existing target directory or set `OPENSSL_SRC_PERL`
to a full Strawberry Perl.
