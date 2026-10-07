# The template standard

Every hook under `templates/` follows the same shape, so a reader can open any one of them and know
where to look. Use it for your own hook too.

## The one rule: the rule is one file

```text
templates/<name>/
  README.md            the rule first, then accounts, errors, how to run, honest limits
  Cargo.toml
  src/
    rule.rs            THE CUSTOM LOGIC. Pure Rust: no accounts, no Solana types. Unit-tested.
    config.rs|state.rs the accounts the hook stores
    instruction.rs     the setup instruction(s)
    error.rs           error codes, one range per template
    processor/         Initialize and Execute, built on hook-kit
    lib.rs
  tests/<name>.rs      runtime tests inside real Token-2022 transfers
```

`rule.rs` opens with the idea in plain words, what the rule guarantees, and **what it does not**. A
function in it takes plain numbers (balances, times, a schedule) and returns a decision. Nothing in
it reads an account, so you can understand and test the rule without understanding Solana.

`processor/execute.rs` is the glue: it validates the call with `hook_kit::execute_prelude`, reads
the numbers the rule needs out of accounts, calls the rule, and writes back any state.

## What `hook-kit` does for you

[`crates/hook-kit`](../crates/hook-kit) is the part every hook needs and nobody should re-derive:

* `execute_prelude`: checks the instruction, the exact account count, that the mint points at this
  program, that both token accounts belong to the mint, that Token-2022 set the `transferring`
  flag (so a direct call is refused), and that the validation list is the canonical one.
* Reading the hooked mint and the token accounts (post-transfer balances, owner).
* `require_extension_authority`, `require_mint_authority_revoked`.
* `create_pda` that survives a pre-funded address, and `create_validation_list`.
* `testing::World`: a hooked Token-2022 mint and funded accounts in-process, and transfers whose
  accounts are resolved through the SDK, exactly as an integrator would.

## Two facts every rule relies on

Both were verified against the Token-2022 source and are covered by tests.

* **Balances are updated before the hook runs.** The hook sees the balance *after* the transfer.
* **`Execute` is built read-only with a non-signer owner.** A hook cannot spend the sender's
  authority. It can sign for its own PDAs.

## Error codes

One range per program, so a failure names its origin. `hook-kit` uses `0x8001..`; creator-commitment
`0xA001..`; fair-launch `0xB001..`; loyalty-rewards `0xC001..`. The reference hook uses `0x7001..` and
the arbitrary test hook `0x9001..`.

## Tests

* **Unit tests** for `rule.rs`: every boundary (one under, at, one over), rounding, overflow
  extremes.
* **Runtime tests** with exact error codes (never "some error"), a rollback assertion after every
  refusal, setup validation, the direct-call refusal.
* **Both execution modes.** Native by default; with `SBF_OUT_DIR` set they run the real SBF
  binary: `cargo build-sbf --manifest-path templates/<name>/Cargo.toml --sbf-out-dir target/integration-sbf`.
* **Through Raydium.** A `HookSetup` provider in `crates/raydium-hook-driver/src/hooks/` lets the
  end-to-end flows run the hook through CPMM and CLMM, with the hook's own refusals and follow-up
  steps (fund, claim, wait for a window to end).

## Honest limits, every time

Each template README ends with a list of what the rule does **not** stop. Common to all of them:
the program's upgrade authority can replace the rule, and a hook that writes shared state
serialises the transfers that touch it.
