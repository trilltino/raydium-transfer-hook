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

The SDK also tests the real SPL off-chain resolver and CPMM/CLMM instruction framing, including independent transfer slices and invalid frame rejection. These tests do not load Raydium programs. The reference-hook ProgramTest verifies Token-2022 invokes the hook and rolls back a rejected transfer, but runtime tests of the external hook-support CPMM/CLMM builds with hooked mints are still required to prove their CPI forwarding and full transaction behavior.
