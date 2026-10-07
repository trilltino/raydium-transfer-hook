# Raydium Transfer Hooks

Infrastructure for running **any Token-2022 Transfer Hook inside Raydium CPMM and CLMM swaps**,
with no allowlist and no approval from Raydium or from this repository. You write the rule; the
stack resolves each swap leg's hook accounts, frames the swap, and proves the hook ran and that a
refusal rolls everything back.

Raydium is an external program. This repository contains no Raydium source: the hook-aware changes
live in forks pinned by `upstream.lock.toml`.

## Build your own hook

1. **Copy the starter**: `Copy-Item -Recurse templates\transfer-hook-starter my-hook`
2. **Implement your rule** in `src/rule.rs`
3. **Test it**: `cargo test`
4. **Build and deploy**: `cargo build-sbf`, then `solana program deploy ...`
5. **Create a hooked Token-2022 mint** and initialise your hook for it
6. **Run it through Raydium CPMM and CLMM** with `raydium-hook e2e`

The full guide is [`docs/writing-a-hook.md`](docs/writing-a-hook.md). Using the starter is
optional: the repository also ships an unrelated hook (`programs/arbitrary-test-hook`) that runs
through both AMMs with no change to the SDK or the Raydium builders.

**Forking?** Writing and testing a hook needs only a Rust toolchain. Running it through real
Raydium pools needs the two hook-aware Raydium builds made with *your own* keys, because the ones
here bake in ours. [`docs/forking.md`](docs/forking.md) lists what to reuse, what to change and the
steps, and `cargo xtask devnet-doc` regenerates the evidence page for your own deployment.

## Example hooks

Three complete hooks, each in its own folder under `templates/`. In each, **the custom logic is one
short file, `src/rule.rs`**: pure Rust with no accounts and no Solana types, unit-tested on its own.
The rest of the folder is plumbing you can leave alone (the shared parts live in
[`crates/hook-kit`](crates/hook-kit)).

| Folder | The rule | Read |
|---|---|---|
| [`templates/creator-commitment`](templates/creator-commitment) | A creator's allocation **vests**: the dedicated account's balance may not fall below what a cliff-and-linear schedule still locks | [`rule.rs`](templates/creator-commitment/src/rule.rs) |
| [`templates/fair-launch`](templates/fair-launch) | During a launch window, **buys** are limited: per-buy size, per-account balance, buys per slot (against bundles), declared priority fee | [`rule.rs`](templates/fair-launch/src/rule.rs) |
| [`templates/loyalty-rewards`](templates/loyalty-rewards) | Holders earn a **quote-token reward stream** in proportion to balance x time held; the pool never earns | [`rule.rs`](templates/loyalty-rewards/src/rule.rs) |

Each has its own README with the rule, the accounts, every error code and the honest limits, and
each runs through real Raydium CPMM and CLMM pools in the end-to-end flows. The standard they all
follow is in [`docs/writing-a-hook.md`](docs/writing-a-hook.md).

## Status

| Capability | Status | Evidence |
|---|---|---|
| Hooked swap through CPMM (`swap_base_input_v2`), both directions | **Done** | Real CPMM + Token-2022 + hook binaries: in-process runtime test, and on devnet |
| Hooked swap through CLMM (`swap_v3`), both directions | **Done** | Real CLMM pool, tick arrays and position: in-process runtime test, and on devnet |
| Hook refusal aborts the swap, balances and pool state unchanged | **Done** | Asserted for both AMMs, both legs |
| An unrelated hook with no allowlist and no adapter changes | **Done** | `arbitrary-test-hook`: own program id, PDAs, errors; two extras; writes state |
| Starter template | **Done** | Builds and passes standalone, from a copy outside the repository |
| Three example hooks (creator commitment, fair launch, loyalty rewards) | **Done** | Unit, runtime (native and SBF) and Raydium flow tests for each; the flows run through both AMMs in-process and on devnet, see [`docs/devnet.md`](docs/devnet.md) |
| Integration devnet | **Deployed** | Our hook-aware builds under our own program ids; see [`docs/devnet.md`](docs/devnet.md) |
| Official Raydium (including its devnet) | **Not supported** | Their programs do not contain `swap_base_input_v2` / `swap_v3`. No upstream PR has been opened |
| Liquidity deposit / withdraw, fee collection, pool creation with a hooked mint | **Rejected** | Those paths reject hooked mints with a clear error; the hook goes on after a pool has liquidity |
| Creating a pool with a hooked mint | **Needs Raydium's per-mint approval** | Upstream's own mint admission requires a `SupportMintAssociated` record from the pool admin for any mint with a TransferHook. It is per mint, not per hook program: [`docs/permissionless-hooks.md`](docs/permissionless-hooks.md) |
| LaunchLab | **Blocked** | Its on-chain handler is not public; `integrations/launchlab` is a simulator, not an integration |
| Benchmarks of how heavy a hook can be, TypeScript client, commercial templates | **Not done** | |

"In-process" means `solana-program-test`: the real runtime executing the real SBF binaries. There
is no `solana-test-validator` on this Windows setup, so devnet is the real-cluster evidence.

## Layout

| Path | What |
|---|---|
| `crates/transfer-hook-sdk` | Per-leg atomic hook-account resolution, structured errors, framing, V1 goldens |
| `crates/raydium-hook-driver` | Raydium setup builders and the checked end-to-end flows, over RPC or in-process |
| `crates/raydium-hook-cli` | `raydium-hook deploy | e2e | inspect` |
| `crates/hook-policy-model`, `crates/reference-hook-model` | Pure-Rust policy and rule models (not on-chain) |
| `programs/reference-hook-onchain` | The deployable reference hook |
| `programs/arbitrary-test-hook` | An unrelated hook that proves the stack is permissionless |
| `templates/transfer-hook-starter` | Copy-me hook; the rule is `src/rule.rs` |
| `templates/creator-commitment`, `fair-launch`, `loyalty-rewards` | The three example hooks, one folder each; the rule is `src/rule.rs` |
| `crates/hook-kit` | Shared hook plumbing (`Execute` prelude, mint/token reads, PDA creation) and an in-process test world |
| `integrations/` | CPMM and CLMM swap planners and a LaunchLab simulator (models) |
| `environments/` | Cluster manifests: program ids, deployments, evidence |
| `xtask` | `cargo xtask upstream list | verify | fetch` |
| `docs/` | Start at [docs/README.md](docs/README.md): [writing a hook](docs/writing-a-hook.md), [forking](docs/forking.md), [how it works](docs/how-it-works.md), [Raydium instructions](docs/raydium-instructions.md), [limits](docs/hook-limits.md), [security](docs/security.md), [upstream sources](docs/upstream-sources.md), [devnet evidence](docs/devnet.md) |

## Run it

```powershell
# unit + integration tests (native)
cargo test --workspace

# the end-to-end flows against the exact deployed binaries, in-process
cargo build-sbf --manifest-path programs\reference-hook-onchain\Cargo.toml --sbf-out-dir target\integration-sbf
cargo build-sbf --manifest-path programs\arbitrary-test-hook\Cargo.toml --sbf-out-dir target\integration-sbf
foreach ($t in 'creator-commitment','fair-launch','loyalty-rewards') {
  cargo build-sbf --manifest-path templates\$t\Cargo.toml --sbf-out-dir target\integration-sbf
}
# (the CPMM and CLMM integration artifacts are built from the pinned forks: docs/forking.md)
cargo test -p program-test-flows --test local_flows -- --ignored --nocapture

# the same flows on devnet
raydium-hook e2e --env environments\devnet.json --keypair .keys\deployer.json `
  --fee-receiver-keypair .keys\cpmm-fee-receiver.json --amm all --hook all --record
```

Windows note: ProgramTest pulls in a vendored OpenSSL build that needs a complete Perl. If a fresh
build directory fails in `openssl-sys`, reuse an existing target directory or set
`OPENSSL_SRC_PERL` to a full Strawberry Perl.

## Trust model

A hook is an untrusted program and can refuse any transfer. The SDK checks transport correctness
(the mint points at the expected program, the validation list is owned by it and parses, the
resolved accounts carry no unexpected privileges). It cannot judge a hook's economics, who can
change its settings, or whether its program can be upgraded. See
[`docs/security.md`](docs/security.md).
