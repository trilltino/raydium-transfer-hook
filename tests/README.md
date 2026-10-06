# Tests

Run the local model and end-to-end suite with:

```powershell
cargo test --workspace
```

`e2e` currently covers:

- No hook on legacy SPL Token and Token-2022 mints.
- Optional and mandatory launch policy, missing platform hook, and preset without an engine.
- Fresh validation-list fetches and malformed/wrong-owner rejection.
- CPMM swap/deposit/withdraw with zero, one, or two hooked transfer legs.
- CLMM tick/bitmap prefix plus explicitly separated hook ranges.
- LaunchLab setup-before-trading and same-mint graduation persistence.
- Hook rejection before the model commits any transfer effects.

These are in-process behavioral tests. Runtime tests against a concrete Raydium fork and Token-2022 program must still verify CPI account forwarding, actual hook invocation, and Solana transaction rollback.
