# Commercial aspects and technical limits

What each example is for and who pays for it, what a hooked swap costs, where the hard limits are, and
what a hook can never do. Every number here is either measured in this repository (with where), computed
from a stated formula, or marked as not measured.

## What the examples are for

| Example | Who wants it | What it does for them | What it does not do |
|---|---|---|---|
| **Fair launch** (`templates/fair-launch`) | a launch team or a community | caps what one buy, one account and one slot can take; with only the per-slot budget on it is an anti-bundle guard | stop one person using many accounts, stop buys spread across slots, or see a tip paid to a block builder |
| **Creator commitment** (`templates/creator-commitment`) | a project that wants a creator's allocation to vest in public | the dedicated account cannot fall below the unvested amount, and moving it to another wallet does not help | prove the creator holds no other tokens, or stop a burn (see the creator-commitment README) |
| **Holder rewards** (`templates/holder-rewards`) | a project paying holders (a loyalty programme) or spinning a child token off a parent | balance × time accounting that settles inside the transfer; a one-time mode where the allocation is funded once | pay holders who never register, create the child token, or take the fees by itself (funding is a separate step today) |
| **Starter** (`templates/transfer-hook-starter`) | anyone writing their own rule | the plumbing done; the default rule is a per-transfer maximum | enforce anything beyond the rule you write |

A hook constrains **transfers**. It cannot make a mint "safe": anyone can write a hook that refuses
every transfer, and the stack checks transport correctness, not trustworthiness.

## Who pays what

| Cost | Who pays | How much | Source |
|---|---|---|---|
| The hook program | whoever deploys it | refundable rent, about 5.1 SOL per MB; the programs here are 126 to 169 KB, so about 0.7 to 0.9 SOL each | [hook-thickness.md](hook-thickness.md); the starter's devnet deployment cost about 0.94 SOL |
| Per-mint setup (`Initialize`) | the mint's hook authority | rent for the validation list, the config and, for fair-launch, the slot counter. Computed from the rent formula (6,960 lamports per byte including the 128-byte header), fair-launch is about 0.005 SOL per mint; a holder-rewards record is about 0.0014 SOL per registered account | arithmetic, not a measurement |
| Compute for each hooked transfer | the trader, in the swap's compute | see the next table | `benches/`, [hook-thickness.md](hook-thickness.md) |
| Registering a holder (holder-rewards) | whoever calls `Register` | the record's rent, about 0.0014 to 0.0015 SOL | holder-rewards README |
| Approving a hooked mint for pool creation | whoever runs the Raydium deployment | a transaction fee and the record's rent | [forking.md](forking.md) |
| Running a shared devnet | the operator | about 9 SOL of rent for both Raydium programs (the CPMM program account held 3.03 SOL at 597 KB; CLMM is about twice the size) | measured on the deployments in [devnet.md](devnet.md) |

## What a hooked swap costs

From the full benchmark sweep in [`benches/results/results.md`](../benches/results/results.md), run
in-process against the real binaries with a hook that does nothing but declare N extra accounts. A real
rule adds its own compute. The figures move by several thousand units between runs because each run
uses fresh mint keys and `find_program_address` is dearer for a key whose bump is further from 255, so
read them as ranges.

| What | Compute units |
|---|---|
| a bare Token-2022 transfer through a hook with no extras | about 19,000 to 25,000 |
| each PDA-derived extra account | roughly 13,000 to 15,000 |
| a CPMM swap, one hooked leg, no extras | about 60,000 to 65,000 |
| the same with 8 extras | about 150,000 to 160,000 |
| a CPMM swap with both legs hooked, no extras | about 77,000 to 86,000 |
| a CLMM swap, one hooked leg, no extras | about 100,000 |

The example hooks measured earlier in [hook-thickness.md](hook-thickness.md) sit between about 71,000
and 130,000 units for a CPMM swap with one hooked leg.

## Where the limits are

| Limit | Where it bites | Measured in |
|---|---|---|
| **Memory, not packet or compute** | A transfer works up to about **10** PDA-derived extra accounts in the bench hook. The hook program runs out of its 32 KiB heap at 12 and 14 extras; Token-2022 itself runs out at 16 and above. Asking for a larger heap frame did not help in these runs. Literal-address extras are cheaper and were not measured | `benches/results` |
| **Packet size** | A legacy transaction stops fitting at about 24 to 32 extras (1,177 bytes at 24, 1,441 at 32). Two hooked legs run out of room sooner: 8 extras on CPMM, 6 on CLMM | `benches/results` |
| **Address lookup tables** | They shrink the transaction (913 bytes to 298 at 16 extras, 394 at 64) but do not lift the memory limit above. The in-process runtime installs the table directly, so creating one is not measured | `benches/results` |
| **Priority-fee check** | It reads the `SetComputeUnitPrice` instruction. It cannot see a tip to a block builder, and it does not work on v1 transactions (SIMD-0385), where those instructions are no-ops | fair-launch README |
| **Per-slot rules** | A per-slot budget slows a bundle but does not stop many accounts, many slots, or a private bundle | fair-launch README |
| **`max_wallet`** | Per token account, not per person | fair-launch README |
| **Contention** | Fair-launch and holder-rewards write one account on every transfer, so transfers of that mint in a block serialise on it. Measured only on a local single-node validator: 16 simultaneous buys all landed, in the same slot, on one pool or spread over four pools of one mint, with no difference between a read-only and a writing hook, and a per-slot budget held across the four pools (the counter is per mint). That says nothing about a busy real cluster, where the runtime's per-account compute cap per block is what bounds a hot account; spreading buys over pools does not take the shared counter out of that, so four pools of one mint should be expected to reach the cap as soon as one pool would | [`benches/contention`](../benches/contention/README.md) |
| **Burn and owner changes** | Neither is a transfer, so a hook never runs for them | creator-commitment README |

## Raydium's per-mint approval is a business gate

CPMM and CLMM admit a Token-2022 mint with a TransferHook extension to a new pool only if the program's
admin has approved that mint. That is Raydium's existing rule, not something this repository added, and
it is per mint, not per hook program. Whoever deploys the hook-aware forks holds that gate. For the
steps see [forking.md](forking.md#approving-a-hooked-mint), and for the rule itself
[transfer-surface-matrix.md](transfer-surface-matrix.md#raydiums-mint-admission-a-real-gate-and-what-it-is).

## Official Raydium

Raydium's own programs, on devnet or mainnet, do not contain the hook-aware instructions
(`swap_base_input_v2`, `swap_v3` and the others), and no upstream pull request has been opened. Everything
here runs on forks under their own program ids. The forks carry Raydium's code under its own licence;
this repository holds only pointers to them (`upstream.lock.toml`).

## What a hook can never do

* Run for a burn, a mint, or an owner change: only transfers call it.
* Spend the transfer authority: Token-2022 passes every account to it read-only.
* Be trusted because it is permissionless: the stack proves the hook ran and that a refusal rolled the
  swap back, not that the rule is good.

## Not measured

* Contention on a real cluster, and the cost of a shared writable account in a busy block. Simultaneous buys on one pool and across four pools of one hooked mint were run, on a local validator only.
* v1 transactions (SIMD-0385): the pinned `solana-sdk` cannot build them.
* A real cluster: latency, confirmation time, leader scheduling.
* Swaps as v0 transactions with a lookup table, and literal-address extras.
