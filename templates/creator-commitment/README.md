# Creator commitment

A creator's allocation that **vests**. The balance of one dedicated token account may not fall below
the amount still locked by a cliff-and-linear schedule. Everyone else trades freely.

**The rule is in [`src/rule.rs`](src/rule.rs). It is about 100 lines and pure: no accounts, no
Solana types.** Everything else in this folder is plumbing you can leave alone.

```text
locked
  ^
  |#########                      locked_total
  |        #  .
  |        #     .               linear unlock from `start` to `end`,
  |        #        .            nothing before `cliff`
  |        #           .
  +--------+-------------+--> time
         cliff           end
```

## The decision

```rust
// src/rule.rs
pub fn check_outgoing(schedule: &Schedule, now: i64, balance_after: u64) -> Result<(), CommitmentError> {
    if balance_after < schedule.locked_at(now) {
        return Err(CommitmentError::VestingFloorBreached);
    }
    Ok(())
}
```

`processor/execute.rs` calls it only when the source of the transfer is the creator's account.
`balance_after` is the post-transfer balance: Token-2022 moves the tokens before it calls a hook.

## What is in this folder

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Schedule`, `locked_at`, `check_outgoing`. Unit-tested on its own. |
| `src/config.rs` | The per-mint config account (creator account + schedule). |
| `src/instruction.rs` | The one setup instruction, `Initialize`. |
| `src/processor/` | `Initialize` and `Execute`, built on [`hook-kit`](../../crates/hook-kit). |
| `src/error.rs` | Error codes from `0xA001`. |
| `tests/creator_commitment.rs` | Runtime tests inside a real Token-2022 transfer. |

## Accounts

One extra account per transfer, so a swap leg is 3 accounts: the config PDA (`["config", mint]`,
read-only), then the hook program and the validation list.

## Errors

| Code | Name | When |
|---|---|---|
| `0xA001` | `InvalidSchedule` | end not after start, or cliff outside the schedule |
| `0xA002` | `ZeroLockedAmount` | nothing to lock |
| `0xA003` | `CreatorAccountMismatch` | the creator account is for another mint |
| `0xA004` | `InsufficientBalanceAtInit` | the account holds less than it is asked to lock |
| `0xA005` | `VestingFloorBreached` | the transfer would leave the account below the locked amount |
| `0xA006` | `InvalidConfig` | wrong or malformed config account |
| `0xA007` | `InvalidInstruction` | malformed setup instruction |
| `0x8001..` | `hook_kit::KitError` | shared checks: wrong mint, wrong authority, direct `Execute`, ... |

## Run it

```bash
cargo test -p creator-commitment-hook                       # native
cargo build-sbf --manifest-path templates/creator-commitment/Cargo.toml --sbf-out-dir target/integration-sbf
SBF_OUT_DIR=target/integration-sbf cargo test -p creator-commitment-hook   # the real SBF binary
```

Through Raydium (the trader's hooked-token account plays the creator): see
`cpmm_with_the_creator_commitment_template` and `clmm_with_the_creator_commitment_template` in
`tests/program-test/tests/local_flows.rs`. They check that normal swaps pass, a sale that
would breach the floor is refused with `0xA005` and rolls back, and the same sale succeeds once the
schedule has ended.

## Ways around the floor, each run for real

These are tested in `tests/creator_commitment.rs` against the real Token-2022 program, so this table
states what happens, not what we hope happens.

| Attempt | Result |
|---|---|
| Hand the whole account to a new wallet (`SetAuthority` on the account owner) | **The floor holds.** The new owner can move what is above the floor and not one token more (`0xA005`); the old owner has no say. The floor belongs to the account. |
| Approve a delegate for the whole balance and let it transfer | **The floor holds.** A delegate's transfer still goes through the hook, so it is bound exactly like the owner. |
| Burn locked tokens | **Not stopped.** Burn is not a transfer, so Token-2022 never calls the hook. But it only hurts the creator: the balance is then below the floor, so nothing can leave until the schedule has unlocked, and nobody else gains. |
| Put the allocation in a different account | **Not covered, and not detectable by the hook.** The commitment binds one dedicated account. `Initialize` refuses a commitment whose account holds less than `locked_total`, but nothing proves the creator holds no other tokens. See "Disclosed" below. |
| A mint with a permanent delegate | **Not tested here.** Token-2022 invokes the hook for a permanent delegate's transfer like any other, so the floor should apply, but the test world does not build such a mint. |

### "Disclosed"

The commitment is on chain and readable: the schedule is in the config PDA
(`["config", mint]`) and the locked account is the one whose balance it names, so anyone can check
the account's balance against the schedule. What the hook cannot establish is that this account holds
the creator's *whole* allocation. That is a fact about how the supply was distributed at launch, and
the honest way to prove it is to show the distribution, not the hook.

## Honest limits

* **The floor belongs to the token account, not the person.** Moving to another wallet does not
  help (tested above): locked tokens cannot leave the account.
* **Burning is invisible to a hook.** A creator can burn locked tokens (tested above); that only
  hurts them.
* **One creator account per mint.** A team with several wallets needs one commitment per account
  (or a different hook).
* **The program's upgrade authority can replace this rule.** Disclose it or revoke it.
* **Time is cluster time** (`Clock::unix_timestamp`), which can drift a little from wall-clock time.
* **Locked tokens still count as supply.** Nothing here escrows them; they sit in the creator's
  account and are visible there.
