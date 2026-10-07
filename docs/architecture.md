# Architecture

Permissionless Token-2022 Transfer Hooks that run inside Raydium CPMM and CLMM swaps. Raydium is an
external program: no Raydium source lives in this repository (see [source-lock](source-lock.md)).

```
integrator (CLI, app, script)
        |
        v
raydium-hook-driver / transfer-hook-sdk     resolve each transfer leg's hook accounts, frame the swap
        |
        v
Raydium program (hook-aware build)          swap_base_input_v2 / swap_v3, forwards each slice
        |
        v
Token-2022 transfer CPI
        |
        v
any Transfer Hook program                   reference hook, arbitrary hook, or yours
```

## Crates and programs

| Path | Role |
|---|---|
| `crates/transfer-hook-sdk` | Official-SPL-based, per-leg, atomic resolution (`resolve/`), structured errors (`error/`), framing into the hook-aware Raydium instructions (`frame/`), V1 builders and golden ABI fixtures (`abi.rs`). |
| `crates/raydium-hook-driver` | Instruction builders for the Raydium admin/pool/position setup and the checked end-to-end flows (`flow/`), over a `Chain` that is a real RPC endpoint or in-process ProgramTest (`chain/`). |
| `crates/raydium-hook-cli` | `raydium-hook deploy | e2e | inspect`: a thin shell over the driver. |
| `crates/hook-policy-model`, `crates/reference-hook-model` | Pure-Rust models of platform policy and hook rules. Not on-chain. |
| `programs/reference-hook-onchain` | The deployable reference hook (max transfer), with the plumbing a hook needs. |
| `programs/arbitrary-test-hook` | An unrelated hook: own program id, PDAs, errors and init; two resolved extras; mutates state. Exists to prove nothing in the stack is specific to the reference hook. |
| `templates/transfer-hook-starter` | The reference hook as a copy-me project; the rule lives in `src/rule.rs`. |
| `templates/creator-commitment`, `fair-launch`, `loyalty-rewards` | Three example hooks, one folder each, with the custom logic in `src/rule.rs`; see [`template-standard.md`](template-standard.md). |
| `crates/hook-kit` | What every hook needs: the `Execute` prelude, mint and token reads, PDA creation, and an in-process test world. |
| `integrations/{cpmm,clmm}` | Plan a swap's two legs and delegate framing to the SDK. Model only. |
| `integrations/launchlab` | A launch-policy simulator. Model only: LaunchLab's handlers are not public. |
| `xtask` | `cargo xtask upstream {list,verify,fetch}`: the pinned upstream sources. |

## One transfer, one slice

A hooked transfer needs the hook program, its validation list and N hook-specific extra accounts,
so a leg carries **N + 2** accounts, with N chosen by the hook author. A swap has two transfers,
so it has two slices. They are resolved independently, appended in input-then-output order, and
**never merged, deduplicated or reordered**: Raydium and Token-2022 read each slice by position.
Solana may deduplicate identical keys when it compiles the transaction message; the logical
section boundaries carried in the instruction data are unaffected.

Resolution is fresh on every build and atomic: if the second leg fails, nothing from the first is
applied. The SDK refuses by default any resolved extra that is a signer or writable; an
integrator names the exact accounts it accepts (a stateful hook's own counter, say).

## Versioning

Existing instructions are byte-compatible and unchanged. Two instructions are added where the
account list needs explicit framing: CPMM `swap_base_input_v2` (input slice then output slice) and
CLMM `swap_v3` (tick arrays, bitmap, input slice, output slice). Every other path rejects a mint
that carries a TransferHook with a clear error instead of an opaque Token-2022 failure. That means
liquidity deposits and withdrawals do not accept hooked mints yet. See the
[transfer-surface matrix](transfer-surface-matrix.md).

## Environments

Program ids are never constants in code. Each environment is a manifest in `environments/`:

| Manifest | Programs | Used for |
|---|---|---|
| `devnet.json` | Our hook-aware Raydium builds (`integration` feature) under our own ids, plus the two hooks | The hooked-swap demo on a real cluster |
| `localnet.json` | The same artifacts, in-process | `crates/raydium-hook-driver/tests/local_flows.rs` |
| `official-devnet.json` | Raydium's own devnet programs | Compatibility checks only. They do not contain the hook-aware instructions, and the driver refuses to send them any. |

## What the evidence is

- ProgramTest executes the actual SBF binaries in the Agave runtime, so a rejected swap rolling
  back, hook invocation counts and compute use are observed, not assumed. It is not a validator
  process: there is no `solana-test-validator` on this Windows setup.
- A devnet run (see [integration-devnet](integration-devnet.md)) is the real-cluster evidence.
- The `integrations/` and model crates assert design facts only; they are not runtime evidence.
