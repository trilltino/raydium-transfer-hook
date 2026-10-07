# What bounds a hook

There is no arbitrary maximum rule count. What bounds a hook is the whole transaction on the target
runtime: serialized size, compute, writable-account contention, CPI depth and setup rent. Limits
change with the network, so every figure below is a measurement on a named runtime, not a protocol
constant.

## Accounts (exact)

* No hook: `0` appended accounts for that transfer.
* A hook with `N` resolved extra accounts: `N + 2` appended accounts (the extras, the hook program,
  the validation list), before the transaction compiler deduplicates keys.
* A swap has two transfers: `(N1 + 2) + (N2 + 2)` when both legs are hooked. Each leg's slice is
  kept separate even if keys repeat.

Observed:

| Hook | `N` | Accounts per hooked leg |
|---|---|---|
| reference hook | 1 | 3 |
| creator-commitment | 1 (config) | 3 |
| arbitrary test hook | 2 (policy, stats) | 4 |
| fair-launch | 3 (config, slot counter, instructions sysvar) | 5 |
| loyalty-rewards | 3 (global, source record, destination record) | 5 |

## Transaction size

Every hook above fits a CLMM `swap_v3` in a single legacy transaction. The largest transaction the
flows sent was 1,030 bytes against the 1,232-byte packet (the fair-launch refusal that sends three
swaps at once). The in-process chain enforces the packet limit, because ProgramTest does not, so a
flow cannot pass locally and fail on a cluster. A hook with many more extras will need versioned
transactions and an address lookup table, which this repository does not yet drive.

## Compute

All numbers are `solana-program-test` runs of the SBF binaries (the real runtime, not a validator),
single runs, one hooked leg per swap. They are data points, not a benchmark. Whole-swap compute
units (Raydium + Token-2022 + the hook) as reported by simulation, hooked token in and out:

| Hook | CPMM swap | CLMM swap |
|---|---|---|
| reference hook (one amount check, the lightest here) | 75,900 | 98,900 to 101,700 |
| creator-commitment | 85,800 to 85,900 | 102,800 to 105,800 |
| arbitrary test hook (writes a counter) | 100,900 to 101,000 | 126,900 to 129,700 |
| fair-launch (reads the instructions sysvar, writes a counter) | 94,300 to 101,500 | 129,000 to 132,100 |
| loyalty-rewards (settles two records and the global) | 116,000 to 116,100 | 127,000 to 129,800 |

Every figure is under the default 200,000-unit limit of a transaction. The reference hook's `Execute`
alone is about 16,600 units on SBF. The driver submits swaps with a 1,400,000-unit limit; a
fee-optimal limit was not explored. Pool state differs between runs, so differences of a few
thousand units between rows are noise, not a ranking.

## Contention

`fair-launch` and `loyalty-rewards` write one account on every transfer (a slot counter; the global
reward account), so transfers of those mints in the same block serialise on it. That is the real
cost of those two rules, and it has not been measured under load. `creator-commitment` only reads.

## Setup rent

A program's rent is a refundable deposit of about 5.1 SOL per MB. The hooks here are 126 to 169 KB,
so about 0.7 to 0.9 SOL each; most of that weight is the `spl-token-2022` dependency, not the rule.
Per-mint accounts are small (the largest, loyalty-rewards' global, is 185 bytes; each holder record
is 73).

## Not measured

Address-lookup-table behavior, versioned transactions, loaded account data, writable contention
under load, CPI trace limits beyond the observed depth, first-use rent for hooks with large state,
simulation latency, hooks with more than three extra accounts, and two different hooks on the two
legs of one swap. The runnable `e2e` flows are the place to add those measurements; see
[`../benches/README.md`](../benches/README.md).
