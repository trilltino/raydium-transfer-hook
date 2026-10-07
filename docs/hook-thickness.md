# Hook thickness

There is no arbitrary maximum rule count. What bounds a hook is the whole transaction on the
target runtime: serialized size, compute, writable-account contention, CPI depth and setup rent.
Limits change with the network, so any figure below is a measurement on a named runtime, not a
protocol constant.

## Account thickness (exact)

- No hook: `0` appended accounts for that transfer.
- A hook with `N` resolved extra accounts: `N + 2` appended accounts (the extras, the hook program,
  the validation list), before the transaction compiler deduplicates keys.
- A swap has two transfers: `(N1 + 2) + (N2 + 2)` when both legs are hooked. Each leg's slice is
  kept separate even if keys repeat.

Observed:

| Hook | `N` | Accounts per hooked leg |
|---|---|---|
| reference hook | 1 | 3 |
| creator-commitment | 1 (config) | 3 |
| arbitrary test hook | 2 (policy, stats) | 4 |
| fair-launch | 3 (config, slot counter, instructions sysvar) | 5 |
| loyalty-rewards | 3 (global, source record, destination record) | 5 |

Every one of these fits a CLMM `swap_v3` in a single legacy transaction: the largest transaction
the flows sent was 1,030 bytes against a 1,232-byte packet (the fair-launch refusal that sends
three swaps at once). The local chain enforces the packet limit so a flow cannot pass in-process
and fail on a cluster.

## Compute measured so far

All numbers are `solana-program-test` runs of the SBF binaries (the real runtime, not a validator),
single runs, one hooked leg per swap. They are data points, not a benchmark.

Whole-swap compute units (Raydium + Token-2022 + the hook), as reported by simulation in the
in-process flows, hooked token in and out:

| Hook | CPMM swap | CLMM swap |
|---|---|---|
| reference hook (one amount check, the lightest here) | 75,900 | 98,900 to 101,700 |
| creator-commitment | 85,800 to 85,900 | 102,800 to 105,800 |
| arbitrary test hook (writes a counter) | 100,900 to 101,000 | 126,900 to 129,700 |
| fair-launch (reads the instructions sysvar, writes a counter) | 94,300 to 101,500 | 129,000 to 132,100 |
| loyalty-rewards (settles two records and the global) | 116,000 to 116,100 | 127,000 to 129,800 |

Every figure is under the default 200,000 compute-unit limit of a transaction. The reference hook's
`Execute` alone is about 16,600 units on SBF.

The driver submits swaps with a 1,400,000 compute-unit limit; a fee-optimal limit was not explored.
The CLMM and CPMM pools differ in state between runs, so differences of a few thousand units
between rows are noise, not a ranking.

## Not measured

Address-lookup-table behavior, versioned transactions, loaded account data, writable contention
under load (fair-launch and loyalty-rewards write one account on every transfer, so those serialise;
this is the real cost of those two rules and has not been measured under load), CPI trace limits
beyond the observed depth, first-use rent for hooks with large state, simulation latency, hooks
with more than three extra accounts, and two different hooks on the two legs of one swap. The runnable `e2e` flows are the
place to add those measurements; see [`../benches/README.md`](../benches/README.md).
