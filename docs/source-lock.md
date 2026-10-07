# Upstream sources

**Raydium source is not vendored in this repository.** CP-Swap and CLMM are external programs. This
repository records exactly which upstream revisions it was reviewed and tested against in
[`upstream.lock.toml`](../upstream.lock.toml), and fetches them on demand into the git-ignored
`target/upstream/` directory.

```bash
cargo xtask upstream list                   # print locked repositories and revisions
cargo xtask upstream verify                 # validate the lock, confirm every SHA exists remotely,
                                            # and fail if any tracked path looks like copied Raydium source
cargo xtask upstream verify --offline       # lock/format and tracked-tree checks only
cargo xtask upstream fetch                  # cpmm + clmm at base_revision -> target/upstream/<name>
cargo xtask upstream fetch --hook --locked  # cpmm + clmm at the hook-support commit; refuses dirty trees
                                            # and unexpected commits
```

Fetched trees are disposable build inputs. They are built with the upstream repository's own
`Cargo.lock`, Anchor and Solana toolchain, never with this workspace's dependency graph. `verify` is
a path-name heuristic plus a remote-existence check, not a content scan.

## Source-review locks

| Source | Revision | Role |
|---|---|---|
| [`raydium-io/raydium-cp-swap`](https://github.com/raydium-io/raydium-cp-swap) | `b3187ae53a1b95a201f855a59024a12ca8f5b51a` | CPMM base |
| [`raydium-io/raydium-clmm`](https://github.com/raydium-io/raydium-clmm) | `ed1eb41519d5355755f7df52b43fa9610938b60b` | CLMM base |
| [`raydium-io/raydium-sdk-V2`](https://github.com/raydium-io/raydium-sdk-V2) | `cc33ec28a8921a35609e83293e9e07ad830b0779` | GPL-3.0 reference only; never copied (a local, git-ignored copy may exist under `reference/`) |
| [`raydium-io/raydium-cpi`](https://github.com/raydium-io/raydium-cpi) | `115df2779d53bacc7db9d0be2773a4b48a6d372b` | Public CPI interface reference |
| [`solana-program/transfer-hook`](https://github.com/solana-program/transfer-hook) | `ec7063291e968f4b0064e4df0324ff49dcf320df` | Execute ABI, validation PDA, TLV resolver |
| [`solana-program/token-2022`](https://github.com/solana-program/token-2022) | `b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e` | Transfer-hook extension and CPI behavior |

These are source-review locks. They are **not** the Rust crate versions this workspace builds against.

## Hook-support revisions (external)

The Transfer Hook account-forwarding change lives in external forks, on top of the base revisions.
No upstream pull request to `raydium-io` has been opened.

| Program | Fork / branch | Hook-support commit | Adds |
|---|---|---|---|
| CPMM | [`trilltino/raydium-cp-swap`](https://github.com/trilltino/raydium-cp-swap) `transfer-hook-support` | `75ddc09f102c8e3e4424058cee188bf8277949fc` | `swap_base_input_v2`; the `integration` build feature |
| CLMM | [`trilltino/raydium-clmm`](https://github.com/trilltino/raydium-clmm) `transfer-hook-support` | `40291d53d84c6a28991ed966aa2efd261843f662` | `swap_v3`; the `integration` build feature |

Host unit tests (`cargo test --lib --locked`, upstream lockfile): CPMM 26 pass by default and 27 with
`--features integration`; CLMM 204 pass by default and 205 with `--features integration`.

The `integration` feature selects a program id, admin and fee-receiver/owner keys, so the hook-aware
builds can be deployed under ids you control. Default, `devnet` and `localnet` behavior is unchanged,
and combining `integration` with either is a compile error. **The committed ids are ours; a fork
sets its own**: see [forking.md](forking.md). The layouts of the two instructions are in
[transfer-surface-matrix.md](transfer-surface-matrix.md).

## Executable dependency line (this workspace)

| Crate | Exact version |
|---|---|
| `solana-program` | `2.2.1` |
| `solana-program-test` | `2.2.7` |
| `solana-sdk` | `2.2.2` |
| `spl-token` | `7.0.0` |
| `spl-token-2022` | `7.0.0` |
| `spl-transfer-hook-interface` | `0.10.0` |
| `spl-tlv-account-resolution` | `0.10.0` |
| `solana-rpc-client` | `2.2.7` (already in the lock through ProgramTest) |

`Cargo.lock` records the full resolution. Do not upgrade this line opportunistically. ProgramTest
2.2.7 bundles the Token-2022 8.0.0 SBF program, which the runtime tests use to exercise the real
transfer-hook CPI. Raydium's own Anchor/Solana graph is intentionally **not** merged into this
workspace.

## External build toolchains

| Program | Package | Toolchain used |
|---|---|---|
| CPMM (`raydium-cp-swap`) | `raydium-cp-swap`, lib `raydium_cp_swap` | `cargo build-sbf` (solana-cargo-build-sbf 4.0.0, platform-tools v1.53); host tests on cargo 1.96.1 |
| CLMM (`raydium-clmm`) | `raydium-clmm`, lib `raydium_clmm` | same |
| Hooks (this repo) | `reference-hook-onchain`, `arbitrary-test-hook`, the templates | same |

Deployed artifacts are recorded with their SHA-256, size and source revision in
`environments/devnet.json`; rebuilding gives the same hash only if the toolchain matches.

## What is and is not verified

* **Verified by execution:** the hooks against the real Token-2022 processor (success, rejection,
  rollback); CPMM `swap_base_input_v2` and CLMM `swap_v3` hooked swaps in both directions under
  ProgramTest with the real Raydium SBF binaries (the CPMM and CLMM runtime tests, and the driver
  flows in `tests/program-test/tests/local_flows.rs`); seven independent hooks through both AMMs
  with no change to the SDK or builders; the same flows on a real `solana-test-validator`
  (`cargo xtask localnet e2e`, from a clean checkout with no private keys, run by CI); and on devnet
  ([devnet.md](devnet.md)).
* The local profile builds the forks with their `localnet` feature (upstream program ids, a
  throwaway admin); devnet uses their `integration` feature (our ids and keys). Both are built from
  the same locked commits.
* The model crates (`hook-policy-model`, `reference-hook-model`, `integrations/*`) assert design
  facts only and are not runtime evidence.
* LaunchLab: the deployed handler is not public. `integrations/launchlab` is a simulator and says so.
  Real integration is **blocked**.
* Official Raydium (including its devnet) does not contain the hook-aware instructions.
