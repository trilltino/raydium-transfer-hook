# Raydium Transfer Hooks

Infrastructure for running **any Token-2022 Transfer Hook inside Raydium CPMM and CLMM swaps**,
with no allowlist and no approval from Raydium or from this repository. You write the rule; the
stack resolves each swap leg's hook accounts, frames the swap, and proves the hook ran and that a
refusal rolls everything back.

Raydium is an external program. This repository contains no Raydium source: the hook-aware changes
live in forks pinned by [`upstream.lock.toml`](upstream.lock.toml), fetched into the git-ignored
`target/upstream/` when something needs to build them.

## Build your own hook

Needs Rust and the [Solana CLI](https://docs.anza.xyz/cli/install) (Agave 4.0, for `cargo build-sbf`
and `solana-test-validator`). No keys, no network beyond fetching the pinned forks.

```sh
# 1. Copy the starter (or one of the five examples below)
cp -r templates/transfer-hook-starter my-hook        # PowerShell: Copy-Item -Recurse templates\transfer-hook-starter my-hook

# 2. Implement your rule: my-hook/src/rule.rs (and my-hook/setup.json if you change its parameters)

# 3. Test it against the real Token-2022 program
cargo test --manifest-path my-hook/Cargo.toml

# 4. Run it through real Raydium CPMM and CLMM pools on a local validator
cargo xtask localnet build                            # the pinned Raydium forks and this repo's hooks
cargo xtask localnet validator                        # leave running; in another terminal:
cargo run -p raydium-hook-cli -- e2e --env environments/localnet.json \
  --keypair tests/fixtures/localnet/admin.json --keys target/localnet/keys \
  --amm all --hook-dir my-hook
```

Step 4 builds your hook, deploys it, creates a hooked Token-2022 mint, sets your hook up from
`setup.json`, creates real CPMM and CLMM pools, swaps in both directions, makes your hook refuse,
checks every balance rolled back, and prints a PASS/FAIL table derived from what actually happened.
`cargo xtask localnet e2e` does all of it for every hook in the repository in one command.

The full guide is [`docs/authoring-hooks.md`](docs/authoring-hooks.md). Using the starter is
optional: [`programs/arbitrary-test-hook`](programs/arbitrary-test-hook) shares no code with it and
runs through both AMMs, described to the stack only as JSON, with no change to the SDK or the
Raydium builders.

**Devnet.** The same commands run against a real cluster with an environment file and your own
keys; see [`docs/forking.md`](docs/forking.md) (your own deployment) and
[`docs/devnet.md`](docs/devnet.md) (ours, with transactions).

## Example hooks

In each, **the custom logic is one short file, `src/rule.rs`**: pure Rust with no accounts and no
Solana types, unit-tested on its own. The rest is plumbing you can leave alone (the shared parts live
in [`crates/hook-kit`](crates/hook-kit)).

| Folder | The rule |
|---|---|
| [`templates/transfer-hook-starter`](templates/transfer-hook-starter) | The copy-me starter: a maximum transfer size, with per-mint config, authority modes and versioning already done |
| [`templates/creator-commitment`](templates/creator-commitment) | A creator's allocation **vests**: the dedicated account may not fall below what a cliff-and-linear schedule still locks |
| [`templates/fair-launch`](templates/fair-launch) | During a launch window, **buys** are limited: per-buy size, per-account balance, buys per slot, declared priority fee |
| [`templates/anti-bundle`](templates/anti-bundle) | A per-slot budget on **buys from recognised venues**, so a bundle packed into one block is refused |
| [`templates/loyalty-rewards`](templates/loyalty-rewards) | Holders earn a **quote-token reward stream** in proportion to balance x time held; the pool never earns |
| [`templates/parent-spin-off`](templates/parent-spin-off) | Parent holders accrue a **child token** allocation by balance x time; the allocation can be funded exactly once |

Each has its own README with the rule, the accounts, every error code and the honest limits.

## Status

Evidence levels, weakest to strongest: `in-process` (`solana-program-test`, the real runtime and SBF
binaries) < `local validator` (`solana-test-validator`, over RPC) < `integration devnet` (our builds
of the forks, under our program ids) < `official Raydium` (Raydium's own deployment).

| Capability | Status | Evidence |
|---|---|---|
| Hooked swap through CPMM (`swap_base_input_v2`) and CLMM (`swap_v3`), both directions | **Integration devnet** | Real CPMM/CLMM pools, tick arrays and positions; [`docs/devnet.md`](docs/devnet.md) |
| Hook refusal aborts the swap, balances and pool state unchanged | **Integration devnet** | Asserted for both AMMs, both legs |
| Every example hook through both AMMs | **Local validator**; devnet for reference, arbitrary, creator-commitment, fair-launch, loyalty-rewards | `cargo xtask localnet e2e`; anti-bundle and parent-spin-off are not yet deployed to devnet (`cargo xtask env deploy-devnet --hook anti-bundle`, needs the integration keys) |
| An unrelated hook with no allowlist and no adapter changes | **Integration devnet** | `arbitrary-test-hook`, and the JSON-described acceptance tests in `tests/third-party-hook` |
| Starter template built from source and run through both AMMs | **Local validator** | `e2e --hook-dir templates/transfer-hook-starter` |
| Different hooks on the two legs, transfer-fee mints | **In-process** | `tests/program-test` |
| Clean checkout reproduces all of the above without private keys | **CI** | [`.github/workflows/ci.yml`](.github/workflows/ci.yml): forks built with their `localnet` feature and a throwaway admin in [`tests/fixtures/localnet`](tests/fixtures/localnet) |
| Official Raydium (including its devnet) | **Not supported** | Their programs do not contain `swap_base_input_v2` / `swap_v3`; no upstream PR has been opened. `raydium-hook env probe` checks a deployment |
| CPMM exact-output swap, pool creation with a live hook, deposit, withdraw, protocol and fund fee collection | **In-process verified** (`_v2` instructions in the CPMM fork) | Each takes the hook slices of both token transfers, framed like the swaps. [`docs/transfer-surface-matrix.md`](docs/transfer-surface-matrix.md) says exactly what ran where; the original instructions still reject hooked mints |
| CLMM liquidity, positions and fees; CPMM creator-fee collection | **Not supported** (CLMM) / **unit-tested only** (CPMM creator fees) | CLMM rejects hooked mints on those paths with a clear error. CPMM creator-fee collection has a fork instruction and a tested framer, but no runtime test: [`docs/transfer-surface-matrix.md`](docs/transfer-surface-matrix.md) |
| Creating a pool with a hooked mint | **Needs the pool admin's per-mint record** | Upstream's mint admission requires a `SupportMintAssociated` record for any mint with a TransferHook. Per mint, not per hook program: [`docs/permissionless-hooks.md`](docs/permissionless-hooks.md) |
| LaunchLab | **Blocked** | Its on-chain handler is not public; `integrations/launchlab` is a simulator, not an integration |
| Hook-thickness benchmarks (accounts, compute, v0/v1, contention) | **Partial** | Measured compute and sizes in [`docs/hook-thickness.md`](docs/hook-thickness.md); no benchmark suite yet ([`benches/`](benches)) |

## The CLI

`raydium-hook` (crate `crates/raydium-hook-cli`) is a thin shell over the same driver the tests use.
`cargo run -p raydium-hook-cli -- help` lists everything:

| Command | Does |
|---|---|
| `hook build DIR`, `hook deploy`, `hook setup`, `hook inspect MINT` | The author's loop for one hook |
| `mint create [--hook PROGRAM]` | A Token-2022 mint with the TransferHook extension |
| `e2e` | The checked end-to-end flows and the results table (`--hook NAME|all`, `--hook-dir DIR`, `--setup FILE`, `--second-hook`, `--transfer-fee-bps`, `--keep-state`, `--record`) |
| `cpmm swap`, `clmm swap` | Swap on a pool `e2e --keep-state` left: resolve each leg, simulate, explain a refusal, send |
| `inspect MINT`, `env probe` | Transport readiness of a mint; whether a cluster's Raydium programs have the hook-aware instructions |
| `template id / publish / show` | The optional template descriptor standard (metadata, never permission) |
| `deploy` | Deploy every program an environment lists, recording hashes and toolchain |

## Repository automation

```sh
cargo xtask upstream verify            # the pins in upstream.lock.toml exist, nothing Raydium is tracked
cargo xtask localnet build             # fetch the locked forks, build them (`localnet`) and every hook
cargo xtask localnet validator         # solana-test-validator with every program preloaded
cargo xtask localnet e2e               # build, start the validator, run every flow, stop it
cargo xtask env deploy-devnet          # build (`integration`), deploy what is missing, run, record
cargo xtask devnet-doc                 # regenerate docs/devnet.md from environments/devnet.json
```

The in-process tests use the same artifacts: `cargo test -p program-test-flows -p
third-party-hook-acceptance -- --ignored` after `cargo xtask localnet build`
(`RTH_PROFILE=integration` uses the devnet binaries and `.keys/` instead).

## Layout

| Path | What |
|---|---|
| `crates/transfer-hook-sdk` | Per-leg hook-account resolution with the official SPL resolver, structured errors, framing, V1 golden fixtures |
| `crates/raydium-adapters` | Instruction builders for the external Raydium programs |
| `crates/raydium-hook-driver` | Hook setup providers (including the generic JSON one) and the checked end-to-end flows, over RPC or in-process |
| `crates/raydium-hook-cli` | `raydium-hook` |
| `crates/hook-kit` | Shared hook plumbing and an in-process test world |
| `crates/hook-policy-model`, `crates/reference-hook-model`, `crates/hook-template-sdk` | Platform policy model, rule model, template descriptor client |
| `programs/` | The reference hook, the unrelated arbitrary hook, the template descriptor registry |
| `templates/` | The starter and the five example hooks |
| `integrations/` | CPMM and CLMM swap planners and a LaunchLab simulator (models) |
| `environments/` | Cluster manifests: `localnet.json` (keyless), `devnet.json` (integration), `raydium-devnet.json` (official) |
| `tests/` | In-process flows, third-party acceptance tests, the localnet fixtures |
| `xtask` | Upstream locks, localnet, devnet deployment, evidence page |
| `docs/` | Start at [docs/README.md](docs/README.md) |

## Trust model

A hook is an untrusted program and can refuse any transfer. The SDK checks transport correctness
(the mint points at the expected program, the validation list is owned by it and parses, the
resolved accounts carry no unexpected privileges). It cannot judge a hook's economics, who can
change its settings, or whether its program can be upgraded. See
[`docs/security.md`](docs/security.md).
