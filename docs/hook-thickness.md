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

Observed: the reference hook has `N = 1` (3 accounts per leg); the unrelated arbitrary hook has
`N = 2` (4 accounts per leg).

## Compute measured so far

All numbers are `solana-program-test` runs of the SBF binaries (the real runtime, not a validator),
single runs, one hooked leg per swap. They are data points, not a benchmark.

| What | Compute units |
|---|---|
| Reference hook `Execute` alone (max-transfer rule), SBF | about 16,600 |
| CPMM hooked swap, reference hook | about 75,900 |
| CPMM hooked swap, arbitrary hook (2 extras, writes a counter) | about 82,900 |
| CLMM hooked swap, reference hook | about 111,000 to 114,000 |
| CLMM hooked swap, arbitrary hook | about 124,000 to 127,000 |

The driver submits swaps with a 1,400,000 compute-unit limit; real limits and a fee-optimal limit
were not explored.

## Not measured

Serialized transaction size and address-lookup-table behavior, transaction format limits, loaded
account data, writable contention under load, CPI trace limits beyond the observed depth,
first-use rent for hooks with large state, simulation latency, hooks with more than two extra
accounts, and two different hooks on the two legs of one swap. The runnable `e2e` flows are the
place to add those measurements; see [`../benches/README.md`](../benches/README.md).
