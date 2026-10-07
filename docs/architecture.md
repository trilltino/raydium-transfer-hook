# How it works

Permissionless Token-2022 Transfer Hooks that run inside Raydium CPMM and CLMM swaps. Raydium is an
external program: no Raydium source lives in this repository (see
[source-lock.md](source-lock.md)).

```text
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
any Transfer Hook program                   one of the examples, or yours
```

## What is in the repository

| Path | Role |
|---|---|
| `crates/transfer-hook-sdk` | Per-leg, atomic account resolution (`resolve/`), structured errors (`error/`), framing into the hook-aware Raydium instructions (`frame/`), V1 builders and golden ABI fixtures (`abi.rs`). Built on the official SPL helpers. |
| `crates/raydium-hook-driver` | Instruction builders for the Raydium admin, pool and position setup, and the checked end-to-end flows (`flow/`), over a `Chain` that is a real RPC endpoint or in-process ProgramTest (`chain/`). Hook setup providers live in `hooks/`. |
| `crates/raydium-hook-cli` | `raydium-hook deploy \| e2e \| inspect`: a thin shell over the driver. |
| `crates/hook-kit` | What every hook needs: the `Execute` prelude, mint and token reads, PDA creation, an in-process test world. |
| `templates/` | The five example hooks and the starter. The rule of each is `src/rule.rs`. |
| `programs/` | The reference hook and the arbitrary test hook. |
| `crates/hook-policy-model` | A pure-Rust model of platform policy. Not on-chain. |
| `environments/` | Cluster manifests: program ids, deployments, recorded evidence. |
| `xtask` | `cargo xtask upstream {list,verify,fetch}` and `cargo xtask devnet-doc`. |

## One transfer, one slice

A hooked transfer needs the hook program, its validation list and N hook-specific extra accounts, so
a leg carries **N + 2** accounts, with N chosen by the hook author. A swap has two transfers, so it
has two slices. They are resolved independently, appended in input-then-output order, and **never
merged, deduplicated or reordered**: Raydium and Token-2022 read each slice by position. Solana may
deduplicate identical keys when it compiles the transaction message; the logical section boundaries
carried in the instruction data are unaffected.

## How a leg is resolved

`transfer_hook_sdk::resolve_leg` resolves one transfer, fresh, against the chain state its fetcher
returns. It is built on the official `spl-transfer-hook-interface` off-chain helper; the crate does
not reimplement the TLV or seed grammar and derives no non-canonical address.

1. Read the mint. It must be owned by Token-2022 and unpack as a mint.
2. If the mint has no TransferHook extension or no hook program, the leg has no slice.
3. Check the hook program: it must exist, be executable, be owned by an allowed loader, and not be
   Token-2022 or a Raydium program. `ResolveOptions` can pin the exact program and the required
   extension authority.
4. Derive the canonical validation address, fetch it, and require that it is owned by the hook
   program and parses as an `ExtraAccountMetaList` for Execute.
5. Resolve the list for this transfer's exact source, mint, destination, authority and amount.
6. Apply the privilege policy: a resolved extra that is a signer or writable is refused unless the
   integrator named it. The hook program and validation list must be read-only non-signers.
7. Return a `LegHook`: `[resolved extras..., hook program, validation list]` and a fingerprint of the
   mint, program and list it was resolved against.

Nothing is cached. `LegHook::verify_unchanged` re-reads the three accounts so a caller can check that
nothing changed between resolving and signing. Errors are structured, `Clone + PartialEq`, and name
the leg (`LegError { leg, mint, source }`).

`resolve_legs` resolves on a private scratch instruction, so a failure on the second leg leaves the
caller's instruction untouched. Resolution never mutates a caller's instruction; only the framers
do, after validating every leg. The framers (`frame_cpmm_swap_base_input_v2`, `frame_clmm_swap_v3`,
and the `*_or_passthrough` forms that leave a swap with no hooked leg as the byte-identical V1) take
resolved `LegHook`s, never raw account metas, re-derive each leg's validation address, and reject
trailing accounts and privilege conflicts. The layouts are in
[transfer-surface-matrix.md](transfer-surface-matrix.md).

## The SPL contract this rests on

At the reviewed SPL interface revision, the validation list is the PDA `["extra-account-metas",
mint]` under the hook program. `Execute` is the discriminator `[105, 37, 101, 197, 75, 251, 102,
26]` followed by the amount as little-endian `u64`; its account prefix is source, mint, destination,
authority, then the validation list, then the accounts resolved from that list.

Token-2022 moves the tokens, marks the source and destination as mid-transfer, invokes the hook, then
clears the flags. A hook error fails the transfer and Solana's transaction atomicity rolls back the
enclosing instruction. Because Token-2022 finds the hook program and validation list among the
accounts the *caller* supplied, a program that performs the transfer must forward each leg's slice
to its own transfer CPI. That is what the hook-aware Raydium builds do.

## Environments

Program ids are never constants in code. Each environment is a manifest in `environments/`:

| Manifest | Programs | Used for |
|---|---|---|
| `devnet.json` | Our hook-aware Raydium builds (`integration` feature) under our own ids, plus the hooks | The hooked-swap demo on a real cluster |
| `localnet.json` | The same forks built with their `localnet` feature (upstream program ids, a throwaway admin from `tests/fixtures/localnet`), plus the hooks under throwaway ids | `cargo xtask localnet e2e` (a real `solana-test-validator`) and the in-process tests in `tests/program-test`; needs no private key |
| `raydium-devnet.json` | Raydium's own devnet programs | Compatibility checks only. They do not contain the hook-aware instructions, and the driver refuses to send them any |

## What the evidence is

* ProgramTest executes the actual SBF binaries in the Agave runtime, so a rejected swap rolling back,
  hook invocation counts and compute use are observed, not assumed. The in-process chain also
  enforces the 1,232-byte packet limit, because ProgramTest does not, so a flow cannot pass
  locally and fail on a cluster.
* `cargo xtask localnet e2e` runs the same flows on a real `solana-test-validator` over RPC, from a
  clean checkout; CI runs it on every change.
* A devnet run ([devnet.md](devnet.md)) is the public-cluster evidence.
* The model crate (`hook-policy-model`) asserts design facts only; it is not runtime evidence.
