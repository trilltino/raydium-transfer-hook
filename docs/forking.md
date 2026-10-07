# Forking this repository

What you can do with a fork, what you can reuse unchanged, and what you must change. Be clear about
which of the three goals you have, because the cost differs a lot.

| Goal | You need | Effort |
|---|---|---|
| **A. Write and test a hook** | a Rust toolchain | minutes: `cargo test` |
| **B. Run your hook through real Raydium pools, in-process** | the two hook-aware Raydium binaries built with *your* keys | an afternoon |
| **C. Do B on a real cluster and publish the evidence** | B, plus devnet SOL (about 13 SOL of refundable rent for everything, much less for just your own hook) | a day |

## A. Write and test a hook (no keys, no Raydium, no network)

```powershell
git clone <your fork>
cd raydium_transfer_hook
Copy-Item -Recurse templates\transfer-hook-starter my-hook      # or copy one of the three examples
cd my-hook
cargo test                                                      # native
cargo build-sbf                                                 # the deployable program
```

Your rule is `src/rule.rs`. The tests run a real Token-2022 transfer through your hook in-process
and check exact error codes and rollback. Nothing else in this repository is needed for this. See
[writing-a-hook.md](writing-a-hook.md).

To use the shared plumbing and the in-process test world (`hook-kit`), put your hook under
`templates/` as a workspace member, as the three examples are.

## B and C. Run it through Raydium

### What a fork cannot reuse

The hook-aware Raydium programs are built from two external forks with an `integration` feature
that **bakes in a program id, an admin key and a fee-receiver key**. The committed manifests and the
tests use *our* ids, and the keys behind them are git-ignored and never published. So a fork cannot
run the Raydium flows against our binaries: it needs its own.

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
   | `creator-commitment-program.json`, `fair-launch-program.json`, `loyalty-rewards-program.json` | the three examples |

3. **Build the artifacts** into `target/integration-sbf`:

   ```powershell
   cargo xtask upstream fetch --hook --locked
   cargo build-sbf --manifest-path target\upstream\cpmm-hook\programs\cp-swap\Cargo.toml `
     --sbf-out-dir target\integration-sbf -- --features integration
   cargo build-sbf --manifest-path target\upstream\clmm-hook\programs\amm\Cargo.toml `
     --sbf-out-dir target\integration-sbf -- --features integration
   foreach ($p in 'programs\reference-hook-onchain','programs\arbitrary-test-hook',
                  'templates\creator-commitment','templates\fair-launch','templates\loyalty-rewards') {
     cargo build-sbf --manifest-path $p\Cargo.toml --sbf-out-dir target\integration-sbf
   }
   ```

4. **Run the flows in-process** (no network):

   ```powershell
   cargo test -p raydium-hook-driver --features local --test local_flows -- --ignored --nocapture
   ```

5. **Point an environment at your ids.** Copy `environments/devnet.json`, replace `programs`,
   `admin` and `cpmm_fee_receiver`, and empty `deployments` and `evidence`.
6. **Deploy and run on a cluster:**

   ```powershell
   raydium-hook deploy --env environments\mine.json --keypair .keys\deployer.json `
     --artifacts target\integration-sbf --keys .keys
   raydium-hook e2e --env environments\mine.json --keypair .keys\deployer.json `
     --fee-receiver-keypair .keys\cpmm-fee-receiver.json --amm all --hook all --record
   ```

   `deploy` skips programs that already exist and records each deployment with its SHA-256 and
   transaction. `e2e` exits non-zero if any check fails, and `--record` appends the evidence.
   Hooks with a time window (`creator-commitment`, `fair-launch`, `loyalty-rewards`) really wait on
   a live cluster; tune them with `--vest-seconds`, `--window-seconds`, `--reward-seconds`.
7. **Publish your evidence page:**

   ```powershell
   cargo xtask devnet-doc --env environments\mine.json --out docs\devnet.md
   ```

## Adding your hook to the flows

A hook needs one small provider, not changes to the SDK or the Raydium builders:

1. `crates/raydium-hook-driver/src/hooks/<yours>.rs`: implement `HookSetup` (how to point a mint at
   your hook and initialise your state, which swaps it must refuse and with which error code, which
   writable extras the integrator accepts, and any follow-up steps). Export it from
   `hooks/mod.rs` and `lib.rs`. The three examples are good models.
2. Add your program id under `programs.templates` in your environment manifest.
3. Add an entry to `ARTIFACTS` in `crates/raydium-hook-cli/src/commands/deploy.rs` and a branch in
   `commands/e2e.rs` so the CLI can deploy and run it by name.
4. Add a test next to the others in `crates/raydium-hook-driver/tests/local_flows.rs`.

## Do not change

* **The pinned dependency line** (`solana-program 2.2.1`, `solana-program-test 2.2.7`,
  `solana-sdk 2.2.2`, `spl-token 7.0.0`, `spl-token-2022 7.0.0`, `spl-transfer-hook-interface 0.10.0`,
  `spl-tlv-account-resolution 0.10.0`). It is exact on purpose; see
  [upstream-sources.md](upstream-sources.md).
* **Per-leg resolution.** The two transfers of a swap get independent slices that are never merged,
  deduplicated or reordered. Everything else rests on that.
* **No Raydium source in the repository.** `cargo xtask upstream verify` fails if tracked paths look
  like copied Raydium source; keep it that way and keep the pins in `upstream.lock.toml`.

## Windows note

The in-process runtime pulls in a vendored OpenSSL build that needs a complete Perl. If a fresh
build directory fails in `openssl-sys`, reuse an existing target directory or set `OPENSSL_SRC_PERL`
to a full Strawberry Perl.
