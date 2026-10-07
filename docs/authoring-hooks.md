# Authoring a hook

Anyone can write a Transfer Hook and run it inside Raydium swaps. Nothing here is a permission:
there is no allowlist in Raydium, in this repository, or in any registry. Using the starter is
optional; a hook that shares no code with it behaves the same. The repository's second hook
(`programs/arbitrary-test-hook`) is written independently of the reference hook on purpose, and
passes through both AMMs without any change to the SDK or the Raydium builders.

## What you write, and what you don't

You write the on-chain program: the rule that allows or refuses a transfer. You do not write the
account resolution, the Raydium instruction framing, or the Token-2022 plumbing around it.

| You | The stack |
|---|---|
| The rule | Calling your hook on every transfer (Token-2022) |
| Which extra accounts your rule needs | Reading your validation list and resolving those accounts for each transfer (`transfer-hook-sdk`, built on the official SPL helpers) |
| Who may change your hook's settings | Forwarding each leg's accounts through Raydium (`swap_base_input_v2`, `swap_v3`) |

## The easy path: the starter

```powershell
Copy-Item -Recurse templates\transfer-hook-starter my-hook
cd my-hook
cargo test
cargo build-sbf
```

Edit `src/rule.rs`: `validate_params` (what a creator may configure) and `check_transfer` (allow or
refuse each transfer). The starter README has the full eight-step flow. It is a standalone
project: it builds and its tests pass from a copy outside this repository.

## The things a hook must get right

1. **Implement the SPL `Execute` instruction** and **reject any call that did not come from a
   Token-2022 transfer**: check the `transferring` flag on both the source and the destination
   token account. Without this, anyone can call your hook directly.
2. **Choose your PDA seeds** and declare your extra accounts in the validation list
   (`ExtraAccountMetaList`) with seeds-based metas, so the list is the same for every mint.
3. **Initialise the validation list atomically with your per-mint config**, and verify in your init
   instruction that the mint is a Token-2022 mint, carries a TransferHook extension and points at
   your program. Handle a pre-funded config address (anyone can send lamports to it).
4. **Define who may change your settings** and keep that separate from who may re-point the mint at
   a different hook program. The reference hook offers extension authority, mint authority, an
   explicit authority, or immutable.
5. **Return typed errors**, so an integrator can see that your hook, and which rule, refused a
   transfer.
6. **Keep it bounded.** No loops over holders. Every extra account is added to every hooked
   transfer, and a Raydium swap has two transfers.
7. **Do not try to move the transferred tokens.** Token-2022 gives a hook no authority over them.

## State your hook writes

A hook may write its own accounts during `Execute` (a counter, a timestamp). Mark that account
writable in the validation list. The SDK refuses resolved extras that are writable or signers
unless the integrator names them, so the integrator opts in to exactly your state account:

```rust
let options = ResolveOptions::default()
    .with_expected_hook_program(hook_program)
    .with_privilege_policy(PrivilegePolicy::allowing_writable([stats_account]));
```

## Initialisation is yours

Token-2022 defines how a hook executes, not how it is initialised. The driver treats this as a
provider (`HookSetup` in `crates/raydium-hook-driver/src/hooks.rs`): it says how to point a mint at
your hook and how to initialise your per-mint state, plus the error code your hook returns when it
refuses a transfer. Adding a hook to the end-to-end flows is one small provider.

## Running it through Raydium

1. Deploy your program.
2. Create a Token-2022 mint with the TransferHook extension (hook not yet set).
3. Create the pool and seed its liquidity. Liquidity deposits and withdrawals do not accept hooked
   mints yet, so the hook goes on after the pool has liquidity.
4. Point the mint at your hook and initialise your per-mint state.
5. Resolve each swap leg with `transfer-hook-sdk` and send `swap_base_input_v2` (CPMM) or
   `swap_v3` (CLMM) on a hook-aware Raydium build. Official Raydium, including its devnet, does
   not contain those instructions yet.

The driver does all of this: see `raydium-hook e2e` and `docs/integration-devnet.md`.

## Trust

A hook is an untrusted program and can refuse any transfer, including deliberately. Transport
checks (right program, valid validation list, resolvable accounts) say nothing about whether your
rule is fair, who can change it, or whether the program can be upgraded. Say who holds your
program's upgrade authority, or revoke it.
