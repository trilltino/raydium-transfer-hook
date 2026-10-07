# Benchmarks

What a Transfer Hook costs, as a function of how many extra accounts it needs. The harness is
[`harness/`](harness) (crate `hook-bench`), the hook it measures is
[`programs/bench-hook`](../programs/bench-hook), and the results are in [`results/`](results) as
`results.json` (for tools) and `results.md` (for people).

## What it measures

| Scenario | Question | Recorded |
|---|---|---|
| **A Token-2022 transfer** through a hook with N extras, N = 0 to 64 | How thick can a hook be before the transaction stops fitting? | legacy transaction: bytes, accounts, writable accounts, compute units, whether it fits the 1,232-byte packet. v0 transaction with an address lookup table: the same. The validation list's size and rent. |
| **A Raydium CPMM or CLMM swap** through the hook on one mint, N extras per leg | What does a hooked swap cost? | compute units of the whole swap (in and out), the largest transaction, and whether the flow completed |
| **The same swap with the hook on both mints** | What do two hooked legs cost? | the same |
| **A writable counter** as the first extra | What does shared state add? | the same, with one write per `Execute` |

A configuration that does not work (a legacy transaction over the packet limit, a swap that cannot
fit) is **recorded as a failure with its reason**, not dropped: finding the edge is the point.

The bench hook does nothing but the shared checks and, where stated, one write. So the numbers are
the cost of a hook's *thickness*, not of a clever rule. A real rule adds its own compute on top, and
the example templates' figures are in [`../docs/hook-limits.md`](../docs/hook-limits.md).

## What it does not measure

Stated in every report, because an absence must not be read as a result:

* **Contention under load.** `solana-program-test` runs one transaction at a time. Writable-account
  serialisation is a property of a cluster's scheduler under concurrent load; the `write` rows show
  only what the extra write costs in compute, never any waiting.
* **v1 transactions (SIMD-0385).** The pinned `solana-sdk 2.2.2` cannot build or sign them, and this
  repository does not move its dependency line opportunistically. Measuring v1 needs a deliberate
  compatibility migration first.
* **Swaps as v0 transactions with a lookup table.** The driver's chains send legacy transactions;
  the transfer table shows the v0 effect on the hook's own accounts.
* **A real cluster:** latency, confirmation, leader scheduling.
* Loaded-accounts data size and CPI-depth limits beyond what the runs reach.

## What the full run found

From [`results/results.md`](results/results.md) (in-process, the bench hook's extras are PDAs derived
from three seeds each; literal-address extras are cheaper and were not measured):

* **The limit is memory, not the packet and not compute.** A transfer through the bench hook works up
  to **10 extra accounts** (12 hook accounts) and then fails: the hook program (built on `hook-kit`)
  runs out of its 32 KiB heap at 12 and 14 extras, and **Token-2022 itself** runs out of heap at 16
  and above. Adding `request_heap_frame(256 KiB)` to the transaction did **not** fix either case in
  these runs. Treat roughly ten PDA-derived extras as the practical ceiling today, and measure your
  own hook.
* **Legacy transactions stop fitting at about 24 to 32 extras** (1,177 bytes at 24, 1,441 at 32), but
  heap runs out first. With an address lookup table the same transaction is about 300 bytes at 16 and
  394 at 64 (the lookup table is installed directly in the in-process runtime, so creating one is not
  measured).
* **Compute grows by roughly 13,000 to 15,000 units per PDA extra** (about 19,000 at N = 0 to about
  140,000 at N = 8 for a bare transfer). The figures move by several thousand between runs because
  each run uses fresh random mint keys and `find_program_address` costs more for a key whose bump is
  further from 255; read them as ranges.
* **A hooked swap** costs, with one hooked leg, about 65,000 units at N = 0 and about 160,000 at N = 8
  on CPMM, and about 100,000 and 195,000 on CLMM. With both mints hooked, about 86,000 at N = 0 and
  about 295,000 at N = 8 on CPMM. Swaps stop completing at 10 to 12 extras per leg (heap) or earlier
  when the transaction passes 1,232 bytes (two hooked legs: 8 extras on CPMM, 6 on CLMM).
* **A writable extra** (one shared counter) is not distinguishable from a read-only one in compute:
  the difference is inside the run-to-run noise above. Its real cost, waiting for the account under
  load, is not measured here.

## Run it

```bash
cargo xtask localnet build        # the Raydium forks and every hook, into target/localnet-sbf
cargo build-sbf --manifest-path programs/bench-hook/Cargo.toml --sbf-out-dir target/localnet-sbf
cargo run --release -p hook-bench                    # the full sweep -> benches/results
cargo run --release -p hook-bench -- --quick         # a few configurations, to see it work
cargo test -p hook-bench -- --ignored                # the quick sweep as a test (CI runs this)
```

`hook-bench` builds the bench hook for you if `bench_hook.so` is missing. Every figure is a
measurement on the runtime and toolchain recorded at the top of `results.md`, with the SHA-256 of
each artifact, so it can be reproduced or distrusted. They are not protocol constants: change the
Agave version and they move.

Do not publish model timings (anything from `hook-policy-model`) as on-chain overhead. Only these runs, executing the real binaries, are
measurements.
