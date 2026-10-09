# Creator Commitment

A creator can transfer unlocked tokens but cannot reduce the committed account below its current
vesting floor.

**The rule is [`src/rule.rs`](src/rule.rs). It is short and pure: no accounts, no Solana types.** The
rest of the folder is plumbing.

**Status:** reference implementation, not audited. Read LIMITATIONS and TRUST before using it.

```text
locked
  ^
  |#########                      locked_total
  |        #  .
  |        #     .               linear unlock from `start` to `end`,
  |        #        .            nothing unlocks before `cliff`
  |        #           .
  +--------+-------------+--> time
         cliff           end
```

## WHAT

The balance of **one dedicated token account** (the creator's) may never fall below the amount still
locked by a cliff-and-linear schedule. Everyone else trades freely. It is a *time-dependent,
stateful* rule: the floor is a function of the cluster clock and a config written once.

## WHY

"The team's tokens are vested" is usually a promise. This makes it a property of the token: the
schedule is in an on-chain account anyone can read, and a transfer that would break it fails.

## TRIGGER

Only a transfer whose **source is the creator's committed account**. Transfers between any other
accounts, and transfers *into* the creator's account, are never checked. Burns and owner changes are
not transfers, so a hook never sees them (see LIMITATIONS).

## EXAMPLE

`locked_total = 9,000`, `start = T`, `cliff = T + 100`, `end = T + 300`. The creator's account holds
10,000.

| When | Locked | The creator may send | Result of sending 1,001 |
|---|---|---|---|
| `T + 50` (before the cliff) | 9,000 | up to 1,000 | refused `0xA005` |
| `T + 150` (after the cliff, halfway) | 4,500 | up to 5,500 | allowed |
| `T + 300` (the end) and later | 0 | everything | allowed |

Selling down to *exactly* the floor is allowed; one token below is refused.

## RULES

```rust
// src/rule.rs
pub fn check_outgoing(schedule: &Schedule, now: i64, balance_after: u64) -> Result<(), CommitmentError> {
    if balance_after < schedule.locked_at(now) {
        return Err(CommitmentError::VestingFloorBreached);
    }
    Ok(())
}
```

* `balance_after` is the post-transfer balance (Token-2022 moves the tokens before the hook runs).
* Before `cliff` the whole `locked_total` is locked. From `cliff` the locked amount falls linearly,
  measured from `start` to `end`, and rounds **up**, so no token unlocks early. From `end` nothing
  is locked.
* `Initialize` refuses an empty schedule, `end <= start`, a cliff outside `[start, end]`, a zero
  `locked_total`, a creator account of another mint (`0xA003`), and an account that holds less than
  `locked_total` (`0xA004`).

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

## LIMITATIONS

Each row below is tested against the real Token-2022 program in `tests/creator_commitment.rs`.

| Attempt | Result |
|---|---|
| Hand the whole account to a new wallet (`SetAuthority` on the account owner) | **The floor holds.** The new owner can move what is above the floor and nothing more. The floor belongs to the account. |
| Approve a delegate for the whole balance and let it transfer | **The floor holds.** A delegate's transfer still goes through the hook. |
| Burn locked tokens | **Not stopped.** Burn is not a transfer, so the hook never runs. It only hurts the creator: the balance is then below the floor, so nothing can leave until the schedule unlocks it. |
| Keep part of the allocation in a different account | **Not covered, and not detectable by the hook.** The commitment binds one dedicated account. `Initialize` checks the account holds `locked_total`, but nothing proves the creator holds no other tokens. |
| A mint with a permanent delegate | **Not tested here.** Token-2022 calls the hook for a permanent delegate's transfer like any other, so the floor should apply, but the test world does not build such a mint. |

Also:

* **The floor belongs to the token account, not the person.** One creator account per mint; a team
  with several wallets needs one commitment per account (or a different hook).
* **It cannot prove the commitment covers the creator's whole allocation.** That is a fact about how
  the supply was distributed at launch; show the distribution, not the hook.
* **Time is cluster time** (`Clock::unix_timestamp`), which can drift from wall-clock time.
* **Locked tokens still count as supply.** Nothing escrows them; they sit in the creator's account.

## TRUST

* **Config authority:** `Initialize` is signed by the mint's live Transfer Hook authority. The
  schedule **cannot be changed afterwards**: there is no update instruction, and a second
  `Initialize` is refused.
* **Program upgrade authority:** can replace this rule for every mint that uses the program. Disclose
  it or revoke it (`solana program set-upgrade-authority <ID> --final`).
* **Mint hook selection:** the mint's Transfer Hook authority can point the mint at a different hook
  and so remove the floor. Revoke the extension's authority to make the commitment durable, and say
  that you did.
* The commitment is readable on chain: the schedule is in the config PDA and the account it binds is
  named there, so anyone can check the balance against the schedule.

## STATE / COST

One extra account per transfer: the config PDA `["config", mint]`, **read-only**. Nothing is written
by the hook, so there is **no writable-account contention**. A transfer leg carries 3 hook accounts
(config, hook program, validation list). The rule itself is a handful of integer operations; the
whole `Execute` measured about 8k to 16k compute units in-process against the SBF build (a
measurement, not a guarantee). The config is 105 bytes and the validation list 51.

## TESTS

`cargo test` from this directory. Unit tests are in `src/rule.rs` and `src/config.rs`; runtime tests
in `tests/creator_commitment.rs` run inside real Token-2022 transfers.

| Behaviour | Test |
|---|---|
| valid transfers | `before_the_cliff_the_creator_can_only_move_what_is_above_the_floor`, `tokens_unlock_linearly_after_the_cliff`, `after_the_end_everything_can_leave` |
| exact boundary | `selling_down_to_exactly_the_floor_is_allowed_and_one_below_is_not` |
| rejection with exact code | the same runtime tests (`0xA005`, balances unchanged) |
| schedule maths | `everything_is_locked_before_the_cliff`, `unlocking_is_linear_from_the_start_once_the_cliff_passes`, `locked_amount_rounds_up_so_no_token_unlocks_early`, `the_locked_amount_never_increases_and_never_exceeds_the_total`, `the_arithmetic_cannot_overflow_at_the_extremes`, `a_schedule_spanning_the_whole_i64_range_is_evaluated_exactly` (rule), `a_schedule_spanning_the_whole_i64_range_still_enforces_the_floor` (runtime) |
| malformed config / schedule | `config_round_trips_and_rejects_bad_shapes`, `validation_rejects_empty_backwards_and_misplaced_cliff_schedules`, `initialize_rejects_bad_schedules_and_balances_with_exact_codes` |
| unauthorized setup, re-init | `initialize_rejects_bad_schedules_and_balances_with_exact_codes` (`0x8004`, `0x8006`) |
| irrelevant transfer path | `other_holders_are_unaffected_and_the_creator_can_receive` |
| bypass attempts | `handing_the_account_to_a_new_owner_keeps_the_floor`, `a_delegate_cannot_move_the_locked_tokens_either`, `burning_locked_tokens_is_not_stopped_and_strands_the_creator_until_the_end` |
| direct `Execute` call | `a_direct_execute_call_is_refused` |
| unauthorized *config update* | not applicable: the config cannot be updated |

```sh
cargo test --locked                            # in this directory
cargo build-sbf --sbf-out-dir target/deploy && SBF_OUT_DIR=$PWD/target/deploy cargo test --locked   # real SBF binary
```

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Schedule`, `locked_at`, `check_outgoing` |
| `src/config.rs` | the per-mint config account (creator account + schedule) |
| `src/instruction.rs`, `src/processor/` | the one setup instruction and `Execute`, on [`hook-kit`](../../hook-kit) |
| `src/error.rs` | error codes from `0xA001` |

## DEPLOY / INITIALIZE

**Evidence so far:** in-process and SBF in-process tests (CI runs both). This template ships no
`examples/devnet.rs`, so this repository has not initialised or exercised it on devnet.

1. **Build and deploy.** `scripts/deploy.sh templates/creator-commitment` builds with
   `cargo build-sbf` and deploys to devnet, then stops: it does not create a mint or initialise
   anything.
2. **Create the mint and the creator account.** A Token-2022 mint whose Transfer Hook extension
   points at the program id, and the creator's token account of that mint holding at least
   `locked_total`.
3. **Initialise.** Send `Initialize(Schedule)` (builder: `instruction::initialize`). Accounts, in
   order: payer (signer, writable), the mint's Transfer Hook authority (signer), mint, creator
   account, config PDA `["config", mint]` (writable), validation list `["extra-account-metas", mint]`
   (writable), system program. Only the mint's live Transfer Hook authority can sign it, and only once.
4. **What it creates.** The config (105 bytes: creator account and schedule) and the validation list
   (51 bytes, equal to `config::VALIDATION_LIST`).
5. **Verify.** Decode the config (`Config::decode`) and check the creator account and schedule; check
   the list bytes; then send one transfer from the creator account that breaches the floor and see
   it refused with `0xA005`. Revoke the mint's Transfer Hook authority if the commitment must be
   durable.

`tests/creator_commitment.rs` (`committed`) builds exactly this arrangement in-process.
