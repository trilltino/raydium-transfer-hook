# templates

Reviewed Transfer Hooks that show what you can build, and the bar a new one has to clear.

**Where it fits.** The repository takes an idea for token business logic to a deployed hook:
write it in `rule.rs`, test it, deploy it, contribute it. [`starter/`](../starter) is the blank
version you copy. These folders are finished examples of the same shape, so you can see a real rule,
its tests, and an honest README before writing your own. A good one of yours can be added here.

| Template | Class of hook | What it enforces |
|---|---|---|
| [`fair-launch`](fair-launch) | allow / reject policy | launch participation controls: caps on buy size, balance and buys per slot, and a priority-fee limit, inside a launch window. Basic bundle and snipe resistance, **not** complete bundle detection |
| [`creator-commitment`](creator-commitment) | time-dependent restriction | a creator account cannot fall below its current vesting floor |
| [`holder-rewards`](holder-rewards) | stateful economic accounting | holders earn a reward token by balance x time, using a global reward index (no loop over holders) |

## Every template has the same layout

```text
<name>/
  README.md        WHAT, WHY, TRIGGER, EXAMPLE, RULES, LIMITATIONS, TRUST, STATE / COST, TESTS
  Cargo.toml
  src/rule.rs      the business logic, pure where possible
  src/             config, instruction, processor, error codes (built on ../hook-kit)
  tests/           runtime tests inside real Token-2022 transfers
```

Run one: `cd templates/fair-launch && cargo test`. Run all of them: `cargo test --workspace` from the
repository root.

## Adding one

The full checklist is in [`CONTRIBUTING.md`](../CONTRIBUTING.md). In short: a problem statement,
a `rule.rs`, positive, rejection and boundary tests, the extra and writable accounts, who controls
the config and whether the program can be upgraded, how the rule can be bypassed, and a deployment
run. The repository curates templates; it does not accept every working piece of Rust.

Not ready for a template yet? [`IDEAS.md`](IDEAS.md) lists ideas with what stands between each and a
template. Add yours there first.
