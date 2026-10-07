# Benchmarks

No benchmark suite exists yet. The only measurements are the compute-unit data points recorded in
[`../docs/hook-thickness.md`](../docs/hook-thickness.md), taken from `solana-program-test` runs of
the SBF binaries (the real runtime, not a validator, one run each).

The driver already records what a benchmark needs: each swap is simulated before it is sent and the
simulation's compute units, log count and hook-invocation count are kept in the run evidence
(`raydium-hook e2e --record`). A real benchmark should add:

- Solana/Agave version, cluster configuration, program ids and artifact hashes.
- Instruction data, fixed accounts, remaining-account ranges, extension state and validation-list size.
- Serialized message bytes, account and writable counts, CPI count and depth, address-lookup-table use.
- First-use account creation and rent, and latency.
- Hooks with more extra accounts, a different hook on each leg, and writable shared state under load.

Do not publish model timings as on-chain overhead.
