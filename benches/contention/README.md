# Contention: simultaneous hooked swaps, on one pool and across several pools of one mint

Questions: a hook that writes shared state (fair-launch writes one counter account per mint on every buy)
takes a write lock that a read-only hook does not. (1) Do simultaneous trades on one pool suffer for it?
(2) With several pools of the same hooked mint, is the hook's per-mint account the one thing the pools
share, and does anything change when trades are spread over them?

`contention.ts` sends K buys at once, from K different wallets, and times each from its own send to its
confirmation. It runs these scenarios against the same validator:

| Scenario | Hook | Pools the buys go to | Shared write beyond the pools |
|---|---|---|---|
| stateless, one pool | the starter (`max transfer`), config read-only | 1 | none |
| stateful, one pool | fair-launch, very large per-slot budget | 1 | one counter account per mint |
| stateless, four pools | the same starter | 4, buyers take them in turn | none |
| stateful, four pools | the same fair-launch | 4, buyers take them in turn | the one counter account, shared by all four pools |
| budget, one pool / four pools | fair-launch, per-slot budget of 3 | 1 / 4 | the counter, which now also refuses |

The four pools of one mint are real CPMM pools of the same hooked and quote mints, each under its own
AmmConfig, all four hooked-token vaults listed as venues of the one hook configuration (the most fair-launch
allows). Both pool and vault accounts are therefore different per pool; the mint, the hook's config and the
hook's counter are the same.

## Run it

```sh
cargo xtask localnet build
cargo xtask localnet validator                      # leave running
cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook reference   --extra-pools 3 --seed-amount 400000000 --out target/contention/stateless.json
cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook fair-launch   --max-buys-per-slot 100000 --extra-pools 3 --seed-amount 400000000 --out target/contention/stateful.json
cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook fair-launch   --max-buys-per-slot 3 --extra-pools 3 --seed-amount 400000000 --out target/contention/budget.json   # optional
NODE_NO_WARNINGS=1 node --experimental-transform-types benches/contention/contention.ts
```

`CONTENTION_K` (default `1,4,8,16`) and `CONTENTION_ROUNDS` (default `5`) change the sweep. It uses the same
client and adapter code as the browser UI. Without `--extra-pools` only the one-pool rows run.

## Result (one run, local single-node `solana-test-validator`, Agave 4.0.0, Docker on Windows)

Five rounds per row; one token per buy; every transaction confirmed at the `confirmed` commitment.

| concurrent buys | hook | pools | landed | median ms | p95 ms | slots used | most in one slot |
|---|---|---|---|---|---|---|---|
| 1 | stateless | 1 | 5/5 | 368 | 468 | 5 | 1 |
| 1 | stateful | 1 | 5/5 | 394 | 461 | 5 | 1 |
| 1 | stateless | 4 | 5/5 | 355 | 454 | 5 | 1 |
| 1 | stateful | 4 | 5/5 | 370 | 435 | 5 | 1 |
| 4 | stateless | 1 | 20/20 | 324 | 422 | 5 | 4 |
| 4 | stateful | 1 | 20/20 | 355 | 372 | 5 | 4 |
| 4 | stateless | 4 | 20/20 | 344 | 395 | 5 | 4 |
| 4 | stateful | 4 | 20/20 | 310 | 400 | 5 | 4 |
| 8 | stateless | 1 | 40/40 | 279 | 340 | 5 | 8 |
| 8 | stateful | 1 | 40/40 | 226 | 339 | 5 | 8 |
| 8 | stateless | 4 | 40/40 | 255 | 335 | 5 | 8 |
| 8 | stateful | 4 | 40/40 | 225 | 314 | 5 | 8 |
| 8 | budget of 3 | 1 | 15/40 | 261 | 319 | 5 | 3 |
| 8 | budget of 3 | 4 | 15/40 | 238 | 340 | 5 | 3 |
| 16 | stateless | 1 | 80/80 | 254 | 517 | 5 | 16 |
| 16 | stateful | 1 | 80/80 | 235 | 563 | 7 | 16 |
| 16 | stateless | 4 | 80/80 | 195 | 210 | 5 | 16 |
| 16 | stateful | 4 | 80/80 | 195 | 513 | 6 | 16 |
| 16 | budget of 3 | 1 | 17/80 | 119 | 612 | 6 | 3 |
| 16 | budget of 3 | 4 | 16/80 | 237 | 548 | 6 | 3 |

## What this shows and what it does not

* Shows: on a local validator, 16 simultaneous hooked buys all succeed and all 16 can be included in the same
  slot, on one pool or spread over four pools of the mint, with or without a hook that writes shared state.
  Confirmation time is set by the slot time (about 400 ms); the differences between rows are inside the
  run-to-run noise (the medians swap order between rows, and the p95 swings from 210 to 612 ms with no pattern
  by hook or by pool count).
* Shows: the hook's state is per mint, not per pool. With a per-slot budget of 3, no more than 3 buys landed in
  any one slot whether the buys went to one pool or were spread over four, and the buys over the budget were
  refused by the hook with its own error (0xB005, `TooManyBuysInSlot`). So a limit you set on a launch applies
  to the token across all its pools, which is what a launch rule should do, and it also means those pools do
  share a write lock on the counter. (The landed counts, 15 or 16 of 40 and 17 or 18 of 80, are the budget
  times the number of slots used, give or take a slot; they are not a throughput figure.)
* Does not show anything about a real cluster. A single-node test validator has one leader, no network, no
  competing traffic and no priority fees, so it cannot say how a busy block's scheduler orders transactions
  that fight over one writable account, how many such transactions fit in a block, or what happens to the
  ones that do not. The runtime caps the compute any one writable account may use per block; on a real
  cluster that cap, not these numbers, bounds how many buys of one launch token one block can take, and a
  counter that every buy writes is subject to it. Spreading the buys over pools takes the pool and vault
  locks off the buys, but not the counter's: if the per-block account cap is the limit that binds on a real
  cluster, four pools of one mint will hit it as soon as one pool would, because the counter is shared. This
  run cannot confirm or refute that; the one measurement that could is a busy cluster.
* Four pools is the most fair-launch can list as venues; more pools of one mint need a different hook design.
* Includes the hook's compute, which is not isolated here: see [`docs/hook-thickness.md`](../../docs/hook-thickness.md)
  and [`benches/results`](../results) for compute per hooked swap.
