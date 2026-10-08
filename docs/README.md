# Documentation

Everything here is about one thing: running **any Token-2022 Transfer Hook inside Raydium CPMM and
CLMM swaps**, with no allowlist, and being able to prove it works. Start with the row that matches
what you are doing.

| I want to... | Read |
|---|---|
| **write a hook** (a rule that allows or refuses transfers) | [authoring-hooks.md](authoring-hooks.md), the [starter](../templates/transfer-hook-starter), then the three examples: [creator-commitment](../templates/creator-commitment), [fair-launch](../templates/fair-launch), [holder-rewards](../templates/holder-rewards) |
| **run hooks through Raydium locally** with no keys | the README's [Build your own hook](../README.md#build-your-own-hook); `cargo xtask localnet e2e` |
| **fork this repo** and run it on my own program ids, or with my own hook | [forking.md](forking.md) |
| **integrate hooked swaps** into an app, wallet or aggregator | [architecture.md](architecture.md), then [transfer-surface-matrix.md](transfer-surface-matrix.md) |
| **judge how heavy a hook can be** (accounts, compute, transaction size) | [hook-thickness.md](hook-thickness.md) |
| **decide whether it is worth it**: who pays, what it costs, where the limits are | [commercial-and-limits.md](commercial-and-limits.md) |
| **assess the risk** of a hook or of this stack | [security.md](security.md) |
| **see what is proven**, with transactions | [devnet.md](devnet.md) |
| **know exactly which Raydium code this is tested against** | [source-lock.md](source-lock.md) |

## The files

| File | What it is |
|---|---|
| [forking.md](forking.md) | What a fork can reuse as-is, what it must change, and the steps to get your own deployment and evidence page |
| [authoring-hooks.md](authoring-hooks.md) | The standard every hook follows: the rule is one file, what must be right, how to test it |
| [architecture.md](architecture.md) | The architecture and how each transfer leg's accounts are resolved and framed |
| [transfer-surface-matrix.md](transfer-surface-matrix.md) | The instructions added to Raydium, their byte layouts, which surfaces are supported, and Raydium's per-mint admission |
| [hook-thickness.md](hook-thickness.md) | What bounds a hook, with measured numbers |
| [commercial-and-limits.md](commercial-and-limits.md) | What each example is for, who pays what, the limits found by the benchmarks, and what a hook can never do |
| [security.md](security.md) | The threat model, what the SDK checks and cannot, malicious-hook tests, and what is not shown |
| [source-lock.md](source-lock.md) | How the Raydium forks are pinned, the dependency line, toolchains, and what is verified |
| [devnet.md](devnet.md) | Evidence, not a guide: what is deployed on devnet under our ids and what ran. A fork does not need it |

## How to read the claims

Every claim in these docs carries its evidence, and a surface is only as supported as the strongest
evidence for it:

`unsupported` < `designed` < `implemented (external branch)` < `in-process verified` <
`local validator verified` < `devnet verified` < `official Raydium deployed`

"In-process" means `solana-program-test`: the real runtime executing the real SBF binaries.
"Local validator" means a `solana-test-validator` process driven over RPC (`cargo xtask localnet
e2e`), from a clean checkout with no private keys; CI runs both. Nothing here is "official Raydium deployed": Raydium's own programs do not
contain the hook-aware instructions, and no upstream pull request has been opened.
