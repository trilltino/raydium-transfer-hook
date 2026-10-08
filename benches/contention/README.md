# Contention: simultaneous hooked swaps

Question: a hook that writes shared state (fair-launch writes one counter account per mint on every buy)
takes a write lock that a read-only hook does not. Do simultaneous trades suffer for it?

`contention.ts` sends K buys at once, from K different wallets, to **one** CPMM pool, and times each from
its own send to its confirmation. Two scenarios run against the same validator and the same pool shape:

| Scenario | Hook | Shared write beyond the pool |
|---|---|---|
| stateless | the starter (`max transfer`), config read-only | none |
| stateful | fair-launch with a very large per-slot budget | one counter account per mint, written on every buy |

Both scenarios write the pool state and the pool's vaults, so the pool is contended either way; what
differs is the hook's own state.

## Run it

```sh
cargo xtask localnet build
cargo xtask localnet validator                      # leave running
cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook reference \
  --out target/contention/stateless.json
cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook fair-launch \
  --max-buys-per-slot 100000 --out target/contention/stateful.json
NODE_NO_WARNINGS=1 node --experimental-transform-types benches/contention/contention.ts
```

`CONTENTION_K` (default `1,4,8,16`) and `CONTENTION_ROUNDS` (default `5`) change the sweep. It uses the same
client and adapter code as the browser UI.

## Result (one run, local single-node `solana-test-validator`, Agave 4.0.0, Docker on Windows)

Five rounds per row; one token per buy; every transaction confirmed at the `confirmed` commitment.

| concurrent buys | hook | landed | median ms | p95 ms | slots used | most in one slot |
|---|---|---|---|---|---|---|
| 1 | stateless | 5/5 | 382 | 418 | 5 | 1 |
| 1 | stateful | 5/5 | 399 | 427 | 5 | 1 |
| 4 | stateless | 20/20 | 209 | 315 | 5 | 4 |
| 4 | stateful | 20/20 | 249 | 270 | 5 | 4 |
| 8 | stateless | 40/40 | 496 | 604 | 6 | 8 |
| 8 | stateful | 40/40 | 570 | 611 | 6 | 8 |
| 16 | stateless | 80/80 | 261 | 735 | 7 | 16 |
| 16 | stateful | 80/80 | 237 | 619 | 6 | 16 |

## What this shows and what it does not

* Shows: on a local validator, 16 simultaneous hooked buys on one pool all succeed, and all 16 can be
  included in the same slot, with or without a hook that writes shared state. Confirmation time is set by
  the slot time (about 400 ms), and the differences between the two hooks in the table are within the
  run-to-run noise (the medians swap order between rows).
* Does not show anything about a real cluster. A single-node test validator has one leader, no network, no
  competing traffic and no priority fees, so it cannot say how a busy block's scheduler orders transactions
  that fight over one writable account, how many such transactions fit in a block, or what happens to the
  ones that do not. The runtime caps the compute any one writable account may use per block; on a real
  cluster that cap, not these numbers, bounds how many buys of one launch token one block can take, and a
  counter that every buy writes is subject to it.
* Does not test the case that matters most for a shared counter: **several pools of the same hooked mint**,
  where an unhooked token's trades on different pools would run in parallel and a hook that writes one
  per-mint account would serialise them. Building that needs more than one pool per mint with the launch
  venues listed in the config; it is not built here.
* Includes the hook's compute, which is not isolated here: see [`docs/hook-thickness.md`](../../docs/hook-thickness.md)
  and [`benches/results`](../results) for compute per hooked swap.
