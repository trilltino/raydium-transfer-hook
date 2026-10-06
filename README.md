# Raydium Transfer Hooks

Developer infrastructure for running **arbitrary, permissionless Token-2022 Transfer Hooks** inside
Raydium CPMM and CLMM transfers. Raydium is an external integration target: this repository holds
the hook framework, resolver/SDK, reference hook and test harness, not Raydium source.

> **Status: early.** The Token-2022 hook path is runtime-tested. Raydium hook support exists as
> external fork branches with unit tests only; no hooked CPMM/CLMM swap has been executed on a
> validator yet, and nothing is deployed to any cluster. See [Status](#status) before relying on anything.

## Build your own hook (target flow)

1. Copy the starter hook
2. Implement your transfer rule
3. Test it
4. Deploy to devnet
5. Create a hooked Token-2022 mint
6. Configure your hook
7. Run it through CPMM
8. Run it through CLMM

Steps 1 and 4-8 need tooling that is **not built yet** (starter template, `raydium-hook` CLI,
devnet environment). Today you can study and run the reference hook
(`programs/reference-hook-onchain`) and the SDK resolver. No allowlist, registry or approval from
Raydium or this repository is required for a hook to work; templates are optional tooling, not permission.

## Status

| Capability | Status |
|---|---|
| Token-2022 transfer through reference hook (success, rejection, rollback) | PASS (ProgramTest, real Token-2022 processor, SBF hook) |
| SDK resolution via official SPL TLV/account-resolution helpers, per transfer | PASS (unit + ProgramTest) |
| CPMM `swap_base_input_v2`, CLMM `swap_v3` handlers | Implemented on external fork branches; host unit tests pass; **no validator execution** |
| CPMM / CLMM hooked swap local E2E | Not done |
| Starter template, setup-provider interface, arbitrary third-party hook proof | Not done |
| Integration devnet / official Raydium devnet | Not done |
| LaunchLab | **Blocked**: deployed handler source is not public; the local crate is a model only |

## Raydium programs and ABI

External hook-support branches (pinned in [`upstream.lock.toml`](upstream.lock.toml)):

| Program | Branch | Hook-aware instruction | V1 behavior |
|---|---|---|---|
| CPMM | [`trilltino/raydium-cp-swap@transfer-hook-support`](https://github.com/trilltino/raydium-cp-swap/tree/transfer-hook-support) | `swap_base_input_v2` | `swap_base_input` unchanged; helper paths reject hooked mints |
| CLMM | [`trilltino/raydium-clmm@transfer-hook-support`](https://github.com/trilltino/raydium-clmm/tree/transfer-hook-support) | `swap_v3` | `swap_v2` unchanged; helper paths reject hooked mints |

Official Raydium deployments do **not** contain these instructions. Per-surface detail:
[transfer-surface matrix](docs/transfer-surface-matrix.md).

## Layout

- `crates/hook-policy-model`: platform hook policy and account metadata types.
- `crates/transfer-hook-sdk`: fresh per-transfer SPL account resolution; CPMM/CLMM instruction framing.
- `programs/reference-hook`: reference rule model; `programs/reference-hook-onchain`: deployable Solana program.
- `integrations/`: CPMM, CLMM, LaunchLab flow **models** (not live Raydium tests).
- `tests/e2e`: policy, resolver and modeled-flow tests.
- `xtask`: `cargo xtask upstream {list,verify,fetch}`.
- `upstream.lock.toml`: pinned external revisions. `docs/`: source lock, matrix, trust boundary, versioning.

There is no Raydium source in this repository. `cargo xtask upstream verify` fails if any appears.

## Run tests

```powershell
cargo test --workspace
cargo build-sbf --manifest-path programs\reference-hook-onchain\Cargo.toml
$env:SBF_OUT_DIR = (Resolve-Path target\deploy).Path
cargo test -p reference-hook-onchain --test token_2022_transfer -- --nocapture
Remove-Item Env:SBF_OUT_DIR
```

### External Raydium programs

```powershell
cargo xtask upstream verify
cargo xtask upstream fetch --hook --locked        # -> target\upstream\cpmm-hook, clmm-hook
cargo test --locked --lib --manifest-path target\upstream\cpmm-hook\Cargo.toml -p raydium-cp-swap
cargo test --locked --lib --manifest-path target\upstream\clmm-hook\Cargo.toml -p raydium-clmm
cargo build-sbf --manifest-path target\upstream\cpmm-hook\programs\cp-swap\Cargo.toml --sbf-out-dir target\raydium-runtime-sbf
cargo build-sbf --manifest-path target\upstream\clmm-hook\programs\amm\Cargo.toml --sbf-out-dir target\raydium-runtime-sbf
```

Raydium builds use the upstream lockfile and toolchain, never this workspace's dependency graph.
Loading the resulting artifacts in ProgramTest (`SBF_OUT_DIR`) only proves they register as
executable programs; it is not evidence of swap execution.

## Trust model

Hooks are untrusted programs. A hook can intentionally reject any transfer. The SDK validates
transport correctness (mint, hook program, validation PDA, TLV, account ordering); it cannot judge a
hook's economics, authority or upgrade policy. Details: [trust boundary](docs/trust-boundary.md).
