# Permissionless hooks

What "permissionless" means here, and the evidence for each part. The project is permissionless only
if every row below holds; each row says how it is shown and where it is not.

| # | Requirement | Evidence |
|---|---|---|
| 1 | A platform can select an arbitrary valid Transfer Hook program | `hook-policy-model`: a platform's hook program is any key; the SDK pins the expected program by key, never by a list. Model only; see [security.md](security.md). |
| 2 | Raydium does not require a hook allowlist | There is no list of hook programs anywhere: the hook-aware instructions forward whatever slice they are given. **But Raydium does gate hooked *mints* at pool creation, and that gate is upstream, not ours:** see "Raydium's mint admission" below. |
| 3 | This repository does not require a hook allowlist | Nothing in `transfer-hook-sdk` or the driver consults a list of hook programs. Search for one: there is none. |
| 4 | Template registration is optional | The registry ([`programs/hook-template-registry`](../programs/hook-template-registry)) is a side table. No flow, no resolver and no builder reads it. |
| 5 | A custom hook may have its own PDA architecture | The five hooks differ: `["config", mint]`, `["holder", token_account]`, `["rewards", mint]`, `["arb-stats", mint]`, ... The SDK resolves them from each hook's own validation list. |
| 6 | A custom hook may have its own initialisation instructions | Each hook has its own setup instruction layout, run through a `HookSetup` provider or a JSON description. |
| 7 | A custom hook may have its own rule engine | Reference hook, arbitrary hook and the five templates are unrelated rules. |
| 8 | The SDK resolves standard execution accounts regardless of template membership | `transfer_hook_sdk::resolve_leg` takes a mint and a transfer; it has no notion of template. |
| 9 | Custom initialisation can be delegated to a setup provider | [`HookSetup`](../crates/raydium-hook-driver/src/hooks/mod.rs), and for a hook known only by program id [`GenericExternalHook`](../crates/raydium-hook-driver/src/hooks/generic.rs), described in JSON. |
| 10 | A third-party hook needs no edit to the CPMM adapter code | The CPMM builder (`raydium-adapters::swap::cpmm_swap_instruction`) takes resolved legs and never names a hook. |
| 11 | ...nor the CLMM adapter code | Same for `clmm_swap_instruction`. |
| 12 | ...nor the core resolver | Every hook above runs through the one resolver. |
| 13 | A third-party hook can be demonstrated on localnet | [`tests/third-party-hook`](../tests/third-party-hook): the arbitrary hook as a compiled `.so` and a JSON description, with **no hook crate in the test's manifest**, through CPMM and CLMM, alone and next to a different hook on the other leg. In-process (`solana-program-test`); and `cargo xtask localnet e2e` runs the starter built from source, known to the stack only by its `setup.json`, on a real `solana-test-validator`. |
| 14 | ...and on the hook-enabled devnet environment | [devnet.md](devnet.md): five hooks through CPMM, and the three originally deployed templates through CLMM too. The third-party JSON-described run is in-process; see its row for what has not run on devnet. |

## Raydium's mint admission (a real gate, and what it is)

Raydium's CPMM and CLMM already restrict which Token-2022 mints can start a pool. In the pinned
upstream CPMM (`utils/token.rs`, `is_supported_mint`) a Token-2022 mint is admitted only if every
extension it carries is on a short list (transfer fee, metadata pointer, token metadata,
interest-bearing, scaled UI amount), **or** a `SupportMintAssociated` record exists for it.
`TransferHook` is not on the list. The record can only be created by the pool admin or a delegated
"create support mint" owner. So:

* **Creating a pool with a hooked mint needs a human approval from whoever runs that Raydium
  deployment**, per mint. This is upstream behaviour that predates this work; the forks only add
  `swap_base_input_v2` and `swap_v3`.
* **The gate is about the mint, not the hook program.** The same record admits a mint pointing at
  any hook (or none yet: the flows approve the mint while its hook is unset and attach the hook
  after the pool has liquidity). Nothing names or ranks a hook program.
* **Once a pool exists,** swaps through it do not consult the record again.

What this means for the claim: *hook programs* are permissionless; *getting a hooked mint into a
Raydium pool* is not, and depends on the operator. On our integration builds the operator is the
deployer key, so every flow here runs `create_support_mint` as admin first. A launchpad that wants
hooked launches without per-mint approval would need that delegated-owner role, or Raydium to
change the rule.

## What is not shown

* **Official Raydium.** Raydium's own programs do not contain the hook-aware instructions, so no
  hook of any kind runs through them. "Permissionless" is demonstrated on forks under our own ids,
  and even there a hooked mint needs the per-mint admission above before it can start a pool.
* **Hostile hooks on a cluster.** The malicious-hook scenarios are tested in-process and in the SDK
  ([security.md](security.md)), not on devnet.
* **That a permissionless hook is a good hook.** Anyone can write one that refuses every transfer.
  The stack checks transport correctness, not trustworthiness.
