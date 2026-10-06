# Hook thickness

Do not impose an arbitrary maximum rule count. Measure the whole transaction on the target runtime and record:

- Total and incremental compute units.
- Serialized message size.
- Total and writable account counts.
- Instruction trace count and CPI depth.
- First-use setup accounts and rent.
- Simulation latency and failure modes.
- Contention from repeated writes to shared state.

Benchmark at least: no hook, no-op hook, max-wallet only, a light fair-launch stack, loyalty checkpoints for sender and receiver, a heavy combined stack, and a dual-hook swap. The resolver's modeled account thickness is deterministic:

- No hook: `0` appended metas for that transfer.
- Hook with `N` resolved extra metas: `N + 2` appended metas (extra metas, hook program, validation list), before message-level key deduplication by the transaction compiler.
- A modeled two-transfer instruction: `(N1 + 2) + (N2 + 2)` appended metas when both legs are hooked; unhooked legs contribute zero. Transfer-specific ranges are preserved even if keys repeat.

These are account-plan counts only. This workspace has no validator benchmark, real CPI execution, serialized transaction measurement, or compute-unit results. Record those only after running a concrete patched program on a named runtime; see [`../benches/README.md`](../benches/README.md).
