# Benchmarks

No runtime benchmark has been run. The workspace tests model planning and rule execution; they do not execute a Solana transaction.

The current plan-level account thickness is documented in [`../docs/hook-thickness.md`](../docs/hook-thickness.md). A runtime report must include:

- Solana/Agave version, cluster/runtime configuration, program ids and build hashes.
- Exact instruction data, fixed accounts, remaining-account ranges, mint extension state, and validation-list size.
- Compute units, transaction message bytes, account/read-write counts, CPI count/depth, and simulation/runtime latency.
- First-use account creation/rent, failure logs, and whether all token and pool state rolled back on rejection.

Required scenarios remain: no hook, no-op hook, max-transfer rule, address rule, optional and mandatory policy, one-hook and dual-hook swaps, hook rejection, and setup/graduation. Do not publish simulated model timings as on-chain overhead.
