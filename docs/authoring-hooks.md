# Writing a hook

Anyone can write a Transfer Hook and run it inside Raydium swaps. Nothing here is a permission:
there is no allowlist in Raydium, in this repository, or in any registry. A hook that shares no code
with anything here behaves the same.

## What you write, and what you don't

You write the rule that allows or refuses a transfer. You do not write the account resolution, the
Raydium instruction framing, or the Token-2022 plumbing around it.

| You | The stack |
|---|---|
| The rule | Calling your hook on every transfer (Token-2022) |
| Which extra accounts your rule needs | Reading your validation list and resolving those accounts for each transfer (`transfer-hook-sdk`, built on the official SPL helpers) |
| Who may change your hook's settings | Forwarding each leg's accounts through Raydium (`swap_base_input_v2`, `swap_v3`) |
| | The shared hook plumbing (`hook-kit`) |

## The standard: the rule is one file

Every hook under `templates/` has the same shape, so a reader can open any one and know where to
look. Use it for yours.

```text
templates/<name>/
  README.md            the rule first, then accounts, errors, how to run, honest limits
  Cargo.toml
  src/
    rule.rs            THE CUSTOM LOGIC. Pure Rust: no accounts, no Solana types. Unit-tested.
    config.rs|state.rs the accounts the hook stores
    instruction.rs     the setup instruction(s)
    error.rs           error codes, one range per hook
    processor/         Initialize and Execute, built on hook-kit
    lib.rs
  tests/<name>.rs      runtime tests inside real Token-2022 transfers
```

`rule.rs` opens with the idea in plain words, what the rule guarantees and **what it does not**. Its
functions take plain numbers (balances, times, a schedule) and return a decision. Nothing in it
reads an account, so the rule can be understood and tested without understanding Solana.
`processor/execute.rs` is the glue: validate the call with `hook_kit::execute_prelude`, read the
numbers the rule needs out of accounts, call the rule, write back any state.

### Examples

| Hook | The rule, in `src/rule.rs` |
|---|---|
| [`creator-commitment`](../templates/creator-commitment) | A creator's allocation vests: the dedicated account's balance may not fall below what a cliff-and-linear schedule still locks |
| [`fair-launch`](../templates/fair-launch) | In a launch window, buys are limited: size, per-account balance, buys per slot, declared priority fee |
| [`loyalty-rewards`](../templates/loyalty-rewards) | Holders earn a quote-token stream in proportion to balance x time held |
| [`transfer-hook-starter`](../templates/transfer-hook-starter) | The reference hook (a per-transfer maximum with several authority modes) as a standalone copy-me crate: `src/rule.rs` is the rule, and it builds from a copy outside the repo |

[`programs/reference-hook-onchain`](../programs/reference-hook-onchain) (max transfer, with several
authority modes) and [`programs/arbitrary-test-hook`](../programs/arbitrary-test-hook) (a per-slot
counter) exist to show the stack handles hooks written independently of each other.

## What `hook-kit` does for you

[`crates/hook-kit`](../crates/hook-kit) is the part every hook needs and nobody should re-derive:

* `execute_prelude`: checks the instruction, the exact account count, that the mint points at this
  program, that both token accounts belong to the mint, that Token-2022 set the `transferring`
  flag (so a direct call is refused), and that the validation list is the canonical one.
* Reading the hooked mint and token accounts (post-transfer balances, owner).
* `require_extension_authority`, `require_mint_authority_revoked`.
* `create_pda`, which survives a pre-funded address, and `create_validation_list`.
* `testing::World`: a hooked Token-2022 mint and funded accounts in-process, and transfers whose
  accounts are resolved through the SDK exactly as an integrator would.

## Two facts every rule relies on

Both were verified against the Token-2022 source and are covered by tests.

* **Balances are updated before the hook runs.** The hook sees the balance *after* the transfer.
* **`Execute` is built read-only with a non-signer owner.** A hook cannot spend the sender's
  authority. It can sign for its own PDAs.

## What a hook must get right

1. **Implement the SPL `Execute` instruction** and **reject any call that did not come from a
   Token-2022 transfer**: check the `transferring` flag on both token accounts. Without this,
   anyone can call your hook directly. (`execute_prelude` does it.)
2. **Choose your PDA seeds** and declare your extra accounts in the validation list
   (`ExtraAccountMetaList`) with seeds-based metas, so the list is the same for every mint.
3. **Initialise the validation list atomically with your per-mint config**, and verify in your init
   that the mint is Token-2022, carries a TransferHook extension and points at your program. Handle
   a pre-funded config address: anyone can send lamports to a deterministic PDA.
4. **Define who may change your settings** and keep that separate from who may re-point the mint at
   a different hook program.
5. **Return typed errors**, so an integrator can see that your hook, and which rule, refused a
   transfer. One code range per program: `hook-kit` `0x8001..`, creator-commitment `0xA001..`,
   fair-launch `0xB001..`, loyalty-rewards `0xC001..`, reference hook `0x7001..`, arbitrary hook
   `0x9001..`.
6. **Keep it bounded.** No loops over holders. Every extra account is added to every hooked
   transfer, and a Raydium swap has two transfers. See [hook-thickness.md](hook-thickness.md).
7. **Do not try to move the transferred tokens.** Token-2022 gives a hook no authority over them.
   (A hook can move *other* tokens that a PDA of its own controls, as loyalty-rewards does when a
   holder claims, but never inside the transfer it is checking.)

## State your hook writes

A hook may write its own accounts during `Execute` (a counter, a record). Mark them writable in the
validation list. The SDK refuses resolved extras that are writable or signers unless the integrator
names them, so the integrator opts in to exactly your accounts:

```rust
let options = ResolveOptions::default()
    .with_expected_hook_program(hook_program)
    .with_privilege_policy(PrivilegePolicy::allowing_writable([counter_account]));
```

A writable account in every transfer means transfers of that mint in the same block serialise on
it. State that cost in your README.

## Tests

* **Unit tests** for `rule.rs`: every boundary (one under, at, one over), rounding, overflow
  extremes.
* **Runtime tests** with exact error codes (never "some error"), a rollback assertion after every
  refusal, setup validation, and the direct-call refusal.
* **Both execution modes.** Native by default. With `SBF_OUT_DIR` set they run the real SBF binary:
  `cargo build-sbf --manifest-path templates/<name>/Cargo.toml --sbf-out-dir target/integration-sbf`.
* **Through Raydium.** A `HookSetup` provider lets the end-to-end flows run the hook through CPMM and
  CLMM. See below.

## Initialisation is yours

Token-2022 defines how a hook executes, not how it is initialised. The driver treats initialisation
as a provider: `HookSetup` in `crates/raydium-hook-driver/src/hooks/mod.rs`. It says how to point a
mint at your hook and initialise your per-mint state, which swaps the hook must refuse (and with
which error code), which writable extras the integrator accepts, and any follow-up steps (wait for a
window to end, fund and claim a reward, check a balance). Adding a hook to the flows is one small
provider; see [forking.md](forking.md).

## Running it through Raydium

1. Deploy your program.
2. Create a Token-2022 mint with the TransferHook extension (hook not yet set).
3. Create the pool and seed its liquidity. Liquidity deposits and withdrawals do not accept hooked
   mints yet, so the hook goes on after the pool has liquidity.
4. Point the mint at your hook and initialise your per-mint state.
5. Resolve each swap leg with `transfer-hook-sdk` and send `swap_base_input_v2` (CPMM) or `swap_v3`
   (CLMM) on a hook-aware Raydium build. Official Raydium, including its devnet, does not contain
   those instructions yet.

The driver does all of this: `raydium-hook e2e`.

## Say what your rule does not stop

Each example README ends with the limits of its rule. Do the same. What a hook cannot see is as
important as what it can: a burn is not a transfer, so the hook never sees it; a tip paid to a block
builder is not a priority fee. And be honest about trust: a hook is an untrusted program and can
refuse any transfer, including deliberately. Say who holds your program's upgrade authority, or
revoke it. See [security.md](security.md).
