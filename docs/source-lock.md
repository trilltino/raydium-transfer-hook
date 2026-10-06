# Source lock and implementation boundary

**Raydium source is not vendored in this repository.** CP-Swap and CLMM are external programs. This
repository records exactly which upstream revisions it was reviewed and tested against in
[`upstream.lock.toml`](../upstream.lock.toml), and fetches them on demand into the git-ignored
`target/upstream/` directory.

```bash
cargo xtask upstream list                 # print locked repositories and revisions
cargo xtask upstream verify               # validate the lock, confirm every SHA exists remotely,
                                          # and fail if any tracked path looks like copied Raydium source
cargo xtask upstream verify --offline     # lock/format and tracked-tree checks only
cargo xtask upstream fetch                # cpmm + clmm at base_revision -> target/upstream/<name>
cargo xtask upstream fetch --hook --locked  # cpmm + clmm at the hook-support commit; refuses dirty trees
                                            # and unexpected commits
```

Fetched trees are disposable build inputs. They are built with the upstream repository's own
`Cargo.lock`, Anchor and Solana toolchain, never with this workspace's dependency graph.

## Hook repository baseline

| | |
|---|---|
| Repository | `trilltino/raydium-transfer-hook` |
| Baseline revision for this work | `689e3e0a8b1c2ac4b900abc15a232a85254f0afe` |
| Previous baseline (superseded) | `542081e4576c00a3cb74067d1562029b7f8885d0` |

Implementation commits from the baseline forward:

| Commit | Change |
|---|---|
| _(this change set)_ | Remove vendored Raydium trees; add `upstream.lock.toml` and `cargo xtask upstream`; remove the account-union helper from `hook-policy-model` |

## Upstream source-review locks

| Source | Revision | Role |
|---|---|---|
| [`raydium-io/raydium-cp-swap`](https://github.com/raydium-io/raydium-cp-swap) | `b3187ae53a1b95a201f855a59024a12ca8f5b51a` | CPMM base |
| [`raydium-io/raydium-clmm`](https://github.com/raydium-io/raydium-clmm) | `ed1eb41519d5355755f7df52b43fa9610938b60b` | CLMM base |
| [`raydium-io/raydium-sdk-V2`](https://github.com/raydium-io/raydium-sdk-V2) | `cc33ec28a8921a35609e83293e9e07ad830b0779` | GPL-3.0 reference only; never copied |
| [`raydium-io/raydium-cpi`](https://github.com/raydium-io/raydium-cpi) | `115df2779d53bacc7db9d0be2773a4b48a6d372b` | Public CPI interface reference |
| [`solana-program/transfer-hook`](https://github.com/solana-program/transfer-hook) | `ec7063291e968f4b0064e4df0324ff49dcf320df` | Execute ABI, validation PDA, TLV resolver |
| [`solana-program/token-2022`](https://github.com/solana-program/token-2022) | `b5b7511e5d4f19a6a118b858d83a7fe3b0017b1e` | Transfer-hook extension and CPI behavior |

These are source-review locks. They are **not** the Rust crate versions this workspace builds against.

## Hook-support revisions (external)

The Transfer Hook account-forwarding change is developed in external Raydium forks, on top of the
base revisions above, never in a copied tree here.

| Program | External repository / branch | Base | Hook-support commit | Adds |
|---|---|---|---|---|
| CPMM | [`trilltino/raydium-cp-swap`](https://github.com/trilltino/raydium-cp-swap) `transfer-hook-support` | `b3187ae…` | `ec5862d8c735311d6fe88d3d19bd5f3637173db7` | `swap_base_input_v2` |
| CLMM | [`trilltino/raydium-clmm`](https://github.com/trilltino/raydium-clmm) `transfer-hook-support` | `ed1eb41…` | `b04b6ec85a7e85457cafca3fc56511d81beaba06` | `swap_v3` |

These are fork branches; no upstream PR to `raydium-io` has been opened. On each, the host unit
tests pass using the upstream's own lockfile (CPMM: 26 passed; CLMM: 204 passed, 1 ignored,
`cargo test --lib --locked`). The branches contain the changes previously carried in the
now-deleted `vendor/` trees, including the uncommitted shared-helper edits from the working tree at
the time of removal.

Program IDs, discriminators and deployment evidence for these builds belong in
`environments/*.json` once they exist; none is recorded yet.

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

Cargo.lock records the full resolution. Do not upgrade this line opportunistically; change it only
in a dedicated migration that keeps every runtime test green. ProgramTest 2.2.7 bundles the
Token-2022 8.0.0 SBF program, which the runtime tests use to exercise the real transfer-hook CPI.
Raydium's own Anchor/Solana graph is intentionally **not** merged into this workspace.

## External build toolchains

| Program | Package | Toolchain used for last local check |
|---|---|---|
| CPMM (`raydium-cp-swap`) | `raydium-cp-swap`, lib `raydium_cp_swap` | host `cargo test --lib --locked` on cargo 1.96.1; SBF artifacts via `cargo build-sbf` (solana-cargo-build-sbf 4.0.0, platform-tools v1.53) |
| CLMM (`raydium-clmm`) | `raydium-clmm`, lib `raydium_clmm` | same |

Per-artifact records (Cargo.lock hash, artifact hash, program ID) are not yet produced; the
external-build tooling that emits them is future work.

## What is and is not verified

- Verified: reference hook ProgramTest against the real Token-2022 processor (success, rejection,
  rollback); SDK resolution through the official SPL helpers; host unit tests of both external
  hook-support branches.
- A ProgramTest that drives CPMM `swap_base_input_v2` exists as a work in progress in
  `programs/reference-hook-onchain/tests/`; no CPMM or CLMM hooked swap has been executed in a
  runtime test recorded here.
- LaunchLab: the deployed handler is not public. The local LaunchLab crate is a model only and is not
  evidence of deployed behavior. Real integration is **blocked**.
- Nothing has been submitted to a public cluster.
