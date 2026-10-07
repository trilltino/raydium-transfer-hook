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

All numbers are `solana-program-test` runs of the SBF binaries (the real runtime, not a validator)
and a few devnet runs. They are data points, not a benchmark. Whole-swap compute units (Raydium +
Token-2022 + the hook) as reported by simulation, hooked token in and out, as the range seen across
the runs made while writing this. One hooked leg unless stated:

| Hook | CPMM swap | CLMM swap |
|---|---|---|
| reference hook (one amount check, the lightest here) | 71,000 to 76,000 | 99,000 to 102,000 |
| reference hook, TransferFee on both mints | 84,000 | 107,000 to 110,000 |
| creator-commitment | 81,000 to 92,000 | 101,000 to 110,000 |
| arbitrary test hook (writes a counter) | 83,000 to 101,000 | 112,000 to 130,000 |
| anti-bundle (writes a counter) | 95,000 to 99,000 | 110,000 to 111,000 |
| fair-launch (reads the instructions sysvar, writes a counter) | 94,000 to 127,000 | 115,000 to 132,000 |
| loyalty-rewards (settles two records and the global) | 93,000 to 116,000 | 127,000 to 153,000 |
| parent-spin-off (the same accounting) | 129,000 to 130,000 | 125,000 to 128,000 |
| **two hooks, one per leg** (reference + arbitrary) | 113,000 to 137,000 | 147,000 to 157,000 |
| two hooks, the same program on both legs | 112,000 | 136,000 to 139,000 |
| two hooks plus a TransferFee on both mints | 121,000 | 162,000 to 164,000 |

Every figure is under the default 200,000-unit limit of a transaction. The reference hook's `Execute`
alone is about 16,600 units on SBF. The driver submits swaps with a 1,400,000-unit limit; a
fee-optimal limit was not explored. Rows are not a ranking: the same hook varies by tens of
thousands of units between runs, mostly
because every run uses fresh random mints and `find_program_address` costs more compute when the
bump search takes more tries (a hook derives its config PDA on every `Execute`; storing the bump
and using `create_program_address` would make that cost fixed, and has not been done). The highest
figure seen anywhere is about 164,000 (two hooks and a transfer fee on CLMM).

## Transaction formats

Every flow here uses legacy transactions. Two things depend on the format. A hook with many extras
needs a versioned transaction and an address lookup table to fit the packet (the driver does not
drive them yet). And the priority fee: in legacy and v0 transactions it is a `ComputeBudget`
instruction a hook can read; in **v1 transactions (SIMD-0385, live on mainnet since September 2026)**
it is a field of the message and `ComputeBudget` instructions are no-ops, so a hook cannot see it by
reading instructions. See `templates/fair-launch`.

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
