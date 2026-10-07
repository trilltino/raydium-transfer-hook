# Documentation

Everything here is about one thing: running **any Token-2022 Transfer Hook inside Raydium CPMM and
CLMM swaps**, with no allowlist, and being able to prove it works. Start with the row that matches
what you are doing.

| I want to... | Read |
|---|---|
| **write a hook** (a rule that allows or refuses transfers) | [writing-a-hook.md](writing-a-hook.md), then the three examples: [creator-commitment](../templates/creator-commitment), [fair-launch](../templates/fair-launch), [loyalty-rewards](../templates/loyalty-rewards) |
| **fork this repo** and run it on my own program ids, or with my own hook | [forking.md](forking.md) |
| **integrate hooked swaps** into an app, wallet or aggregator | [how-it-works.md](how-it-works.md), then [raydium-instructions.md](raydium-instructions.md) |
| **judge how heavy a hook can be** (accounts, compute, transaction size) | [hook-limits.md](hook-limits.md) |
| **assess the risk** of a hook or of this stack | [trust-model.md](trust-model.md) |
| **see what is proven**, with transactions | [devnet.md](devnet.md) |
| **know exactly which Raydium code this is tested against** | [upstream-sources.md](upstream-sources.md) |

## The files

| File | What it is |
|---|---|
| [forking.md](forking.md) | What a fork can reuse as-is, what it must change, and the steps to get your own deployment and evidence page |
| [writing-a-hook.md](writing-a-hook.md) | The standard every hook follows: the rule is one file, what must be right, how to test it |
| [how-it-works.md](how-it-works.md) | The architecture and how each transfer leg's accounts are resolved and framed |
| [raydium-instructions.md](raydium-instructions.md) | The two instructions added to Raydium, their byte layouts, and which surfaces are supported |
| [hook-limits.md](hook-limits.md) | What bounds a hook, with measured numbers |
| [trust-model.md](trust-model.md) | What the SDK checks, what it cannot, and what the policy models mean |
| [upstream-sources.md](upstream-sources.md) | The pinned Raydium and SPL revisions, toolchains and dependency line |
| [devnet.md](devnet.md) | Generated evidence page: what is deployed on devnet and what ran. Regenerate with `cargo xtask devnet-doc` |

## How to read the claims

Every claim in these docs carries its evidence, and a surface is only as supported as the strongest
evidence for it:

`unsupported` < `designed` < `implemented (external branch)` < `in-process verified` <
`devnet verified` < `official Raydium deployed`

"In-process" means `solana-program-test`: the real runtime executing the real SBF binaries. It is
not a validator process. Nothing here is "official Raydium deployed": Raydium's own programs do not
contain the hook-aware instructions, and no upstream pull request has been opened.
