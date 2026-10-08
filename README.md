# Raydium Transfer Hooks

Run **any Token-2022 Transfer Hook inside Raydium CPMM and CLMM**, with no allowlist and no approval from
Raydium or from this repository. You write the rule; the stack resolves each transfer's hook accounts,
frames the Raydium instruction, and proves that the hook ran and that a refusal rolls everything back.

Raydium is an external program and none of its source is here. The hook-aware changes live in two forks
([`raydium-cp-swap`](https://github.com/trilltino/raydium-cp-swap), [`raydium-clmm`](https://github.com/trilltino/raydium-clmm), branch
`transfer-hook-support`) pinned by [`upstream.lock.toml`](upstream.lock.toml) and fetched into the git-ignored `target/upstream/` when
something has to build them. The original V1 instructions are unchanged; hooks use new `_v2`/`_v3` instructions that take each token's
hook accounts (the real, dynamic `ExtraAccountMetaList`, resolved per leg) as a counted slice.

## What it covers

| | CPMM | CLMM |
|---|---|---|
| Swaps (exact input; CPMM also exact output) | `swap_base_input_v2`, `swap_base_output_v2` | `swap_v3` |
| Pool creation, deposit, withdraw | `initialize_v2`, `deposit_v2`, `withdraw_v2` (and permissioned pools) | positions: `open_position*_v3`, `increase_liquidity_v3`, `decrease_liquidity_v3` |
| Protocol, fund, creator fees | `collect_*_fee_v2` | `collect_protocol_fee_v2`, `collect_fund_fee_v2` (position fees via `decrease_liquidity_v3`) |
| Limit orders | | `open/increase/decrease/settle_limit_order_v2` |
| Reward emissions in a hooked reward mint | | `initialize_reward_v2`, `set_reward_params_v2`, `collect_remaining_rewards_v2`, `decrease_liquidity_v4` |

Each call is verified with the hook live on every token transfer, and an over-limit call is refused by the hook with nothing moved. What
ran where (in-process, local validator, our devnet) is listed per instruction in
[`docs/transfer-surface-matrix.md`](docs/transfer-surface-matrix.md); the devnet transactions are in [`docs/devnet.md`](docs/devnet.md).

**Where it was run, weakest to strongest:** in-process (`solana-program-test` against the SBF binaries) < local validator < **our integration
devnet** (our builds of the forks, under our program ids) < official Raydium. **Official Raydium is not supported**: its deployed programs do
not contain these instructions, and no upstream PR has been opened. LaunchLab is blocked (its on-chain handler is not public).

## Example hooks

In each, **the custom logic is one short file, `src/rule.rs`**: pure Rust with no accounts, unit-tested alone. The shared plumbing is
[`crates/hook-kit`](crates/hook-kit). Each folder has its own README: the rule, the accounts, every error code, the honest limits.

| Folder | The rule |
|---|---|
| [`templates/transfer-hook-starter`](templates/transfer-hook-starter) | The copy-me starter and reference hook: a maximum transfer size |
| [`templates/fair-launch`](templates/fair-launch) | Buys from up to four pool vaults are limited in a launch window: size, balance, buys per slot (**anti-bundle**), declared priority fee (**anti-snipe**) |
| [`templates/creator-commitment`](templates/creator-commitment) | A creator account **vests**: its balance may not fall below what a cliff-and-linear schedule still locks |
| [`templates/holder-rewards`](templates/holder-rewards) | Holders earn a quote-token stream by balance × time; funded once, it is a **parent/child spin-off** |

[`programs/arbitrary-test-hook`](programs/arbitrary-test-hook) shares no code with the starter and runs through both AMMs described only as
JSON: the proof that nothing here is specific to our hooks.

## Build your own hook

Needs Rust and the [Solana CLI](https://docs.anza.xyz/cli/install) (Agave 4.0). No keys. (On Windows, where `solana-test-validator` has no
native build, `cargo xtask localnet validator` uses Docker; Linux and macOS do not need it.)

```sh
cp -r templates/transfer-hook-starter my-hook          # implement my-hook/src/rule.rs
cargo test --manifest-path my-hook/Cargo.toml          # against the real Token-2022 program
cargo xtask localnet build                             # the pinned forks and this repo's hooks
cargo xtask localnet validator                         # leave running; in another terminal:
cargo run -p raydium-hook-cli -- e2e --env environments/localnet.json \
  --keypair tests/fixtures/localnet/admin.json --keys target/localnet/keys --amm all --hook-dir my-hook
```

The last step builds and deploys your hook, makes a hooked mint, creates real CPMM and CLMM pools, swaps both ways, makes your hook
refuse, checks every balance rolled back, and prints a PASS/FAIL table from what happened. `cargo xtask localnet e2e` does it for every hook
here. Guide: [`docs/authoring-hooks.md`](docs/authoring-hooks.md). Devnet: [`docs/forking.md`](docs/forking.md).

## The reference UI

[`apps/fair-launch-ui`](apps/fair-launch-ui) (React + Vite) swaps a hooked token and shows the hook allow or refuse the trade. It
simulates before the wallet signs and is a devnet page. [`docs/frontend.md`](docs/frontend.md) has the detail.

- **Wallets:** a dialog with the wallets' own logos (Phantom, Solflare, other Wallet Standard extensions); it tells the wallet it is on Devnet.
- **Panels** for the three example hooks, with the launch rules in words and try-it buttons for an allowed and an over-limit buy.
- **Transaction trace:** after a swap, every program that ran in order, nested, with compute, accounts, logs, what the hook enforced on
  that transfer, and Solscan links. Devnet is read through our Triton One endpoint, kept server-side in a git-ignored `.env.local`.
- **Getting started with nothing:** *Get SOL*, *Get test tokens* (a dev-server faucet), *Mint to my wallet* (when your wallet is the
  token's mint authority), and under the search bar *Create a demo pool* and *Bring your own token*. These need the dev server on a
  machine with the pool admin's key: creating a pool for a hooked token needs the admin's approval of that token, which a browser wallet cannot give.

```sh
npm install
cargo xtask localnet build && cargo xtask localnet validator     # leave running
npm run ui:dev          # http://127.0.0.1:5173 (devnet); add ?env=localnet and use "Create a demo pool" for a local validator
npm run ui:test         # unit tests; npm run ui:e2e runs the browser suite against a local validator
```

## Honest limits

- **Pool admission is admin-only.** A hooked token needs the pool admin's per-mint record (`raydium-hook mint approve`); there is no allowlist of
  hook programs, but a pool cannot exist without the admin approving the mint.
- **Reward emissions** run for at least seven days, so payout, top-up and reclaim were run only where the clock can be moved (in-process); funding
  also ran on a validator and devnet.
- **Contention** was measured on a local single-node validator only, on one pool and across four pools of one mint; v1 transactions were not
  measured (the pinned `solana-sdk` cannot build them): [`docs/hook-thickness.md`](docs/hook-thickness.md).
- **Not covered:** exact-output swaps and liquidity in the UI, wrapped SOL, pools of an existing token (the tools make the token and pool together).
- **Trust:** a hook is an untrusted program that can refuse any transfer. The SDK checks transport correctness, not a hook's economics or who can
  upgrade it: [`docs/security.md`](docs/security.md).

## Repository map

| Path | What |
|---|---|
| `crates/transfer-hook-sdk` | Per-leg hook-account resolution (the official SPL resolver), structured errors, framing for every instruction above, V1 golden fixtures |
| `crates/raydium-adapters`, `crates/raydium-hook-driver` | Instruction builders for the external programs; the checked end-to-end flows, over RPC or in-process |
| `crates/raydium-hook-cli` | `raydium-hook`: `e2e`, `hook build/deploy/setup/inspect`, `mint create/approve/approval`, `ui-fixture`, `cpmm/clmm swap`, `env probe` (`help` lists all) |
| `crates/hook-kit`, `crates/hook-policy-model` | Shared hook plumbing and an in-process test world; the platform policy model |
| `templates/`, `programs/` | The starter and three example hooks; the unrelated arbitrary hook and the benchmark hook |
| `packages/transfer-hook-client`, `apps/fair-launch-ui` | The TypeScript client (Apache-2.0, checked byte for byte against the Rust goldens); the UI (GPL-3.0-or-later, it uses Raydium SDK V2) |
| `environments/`, `tests/`, `benches/`, `scripts/`, `xtask/` | Cluster manifests (`localnet.json` keyless, `devnet.json` ours); in-process flows, third-party acceptance and localnet fixtures; benchmarks; `approve-hooked-mints.ts`; automation |
| `docs/` | Start at [`docs/README.md`](docs/README.md) |

Automation: `cargo xtask upstream verify` (the pins exist, nothing Raydium is tracked), `localnet build | validator | e2e | ui-fixture`,
`env deploy-devnet`, `devnet-doc`. CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) builds the forks and runs every flow from a clean
checkout with throwaway keys ([`tests/fixtures/localnet`](tests/fixtures/localnet)), including the browser suite.
