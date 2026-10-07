# Parent / spin-off

Holders of a **parent** token earn a one-time allocation of a **child** token, in proportion to how
much parent they hold and for how long, over one window.

**The rule is in [`src/rule.rs`](src/rule.rs).** It is short, because this template is a
*composition*: the balance-time accounting is [`loyalty-rewards`](../loyalty-rewards)' (its `Stream`
and `Holder`), reused unchanged, and the one rule added on top is:

> **The allocation is funded once.** A second funding is refused, so "these child tokens, over this
> window" cannot be quietly extended, diluted or topped up.

(`loyalty-rewards` allows top-ups, because a loyalty programme is ongoing. A spin-off is an event.)

```rust
// src/rule.rs
pub fn check_funding(stream_rate: u64) -> Result<(), SpinOffError> {
    if stream_rate != 0 {
        return Err(SpinOffError::AlreadyFunded);
    }
    Ok(())
}
```

## Transfer semantics, stated explicitly

* **History stays with the historical holder.** What a holder earned up to the moment they sell is
  theirs to claim, whoever holds the parent afterwards.
* **The future follows the parent balance.** From that moment the buyer, if registered, accrues on
  the balance they now hold.
* **Only registered accounts earn, and the pool's vault can never register.**

`src/rule.rs` has unit tests for each of these, and `tests/parent_spin_off.rs` checks them through
real transfers with exact payouts (a 10,000 allocation, a holder who sells everything halfway: 5,000
each, vault left at exactly 0).

## What is in this folder

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: the single-funding check, and the transfer semantics as tests. |
| `src/processor.rs` | A guard in front of `loyalty-rewards`' processor: `Fund` is checked, everything else is delegated. |
| `src/lib.rs` | Re-exports `loyalty-rewards`' instructions and state (same layouts, under this program's id). |
| `src/error.rs` | One error, `0xE001`. Everything else comes from the accounting it reuses. |
| `tests/parent_spin_off.rs` | Runtime tests inside real Token-2022 transfers. |

This is also the pattern for building a hook **out of another hook's crate**: depend on it with the
`no-entrypoint` feature, keep your rule in your own `rule.rs`, and delegate the rest.

## Accounts, instructions and errors

Identical to `loyalty-rewards`: three writable extras per transfer (the global and the two token
accounts' records), so a swap leg is 5 accounts. Instructions are `Initialize`, `Register`, `Fund`,
`Claim`. Errors are `loyalty-rewards`' `0xC001..` (see its README) plus this template's:

| Code | Name | When |
|---|---|---|
| `0xE001` | `AlreadyFunded` | `Fund` after the allocation was already funded |

## Run it

```bash
cargo test -p parent-spin-off-hook                       # native
cargo build-sbf --manifest-path templates/parent-spin-off/Cargo.toml --sbf-out-dir target/integration-sbf
SBF_OUT_DIR=target/integration-sbf cargo test -p parent-spin-off-hook   # the real SBF binary
```

Through Raydium: `cpmm_with_the_parent_spin_off_template` and
`clmm_with_the_parent_spin_off_template` in `tests/program-test/tests/local_flows.rs`. They settle the
registered parent holder through swaps, fund the allocation, check a second funding is refused with
`0xE001`, let the window pass, claim, and confirm the child tokens arrived.

## Honest limits

* **Registration is explicit.** A parent holder who never registers earns nothing (about 0.0015 SOL
  of rent each). See `loyalty-rewards` for why registration is not created inside the swap.
* **The pool earns nothing; other pools and contracts might.** Only the configured pool vault is
  refused. Another pool's vault could register and collect holders' share.
* **The child must be a plain token.** A child with a Transfer Hook of its own (a nested hook) is
  refused at `Initialize` with `0xC009`: its claims would need extra accounts this program does not
  forward.
* **The window is chosen once, by whoever funds.** Fund from an account you trust to choose it, or
  have the program's upgrade authority revoked.
* **Burning is invisible to a hook.** Earnings are capped by the actual balance when settled; see
  `loyalty-rewards`.
* **Contention.** The global is a writable account in every transfer where either side is
  registered, so those transfers serialise on it.
* **The program's upgrade authority can replace this rule.** Disclose it or revoke it.
