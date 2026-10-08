# Holder rewards

Holders earn a reward stream in another token (for example the quote token) in proportion to
**balance x time held**. The loyal holder earns more than the one who arrived late, and buying right
before a payout earns almost nothing.

**The rule is in [`src/rule.rs`](src/rule.rs). It is pure: no accounts, no Solana types.**
Everything else in this folder is plumbing you can leave alone.

## Two modes: an ongoing programme, or a one-time spin-off

`Initialize` takes a mode:

| Mode | Funding | Use |
|---|---|---|
| **ongoing** (default) | can be topped up: funding again extends the stream | a loyalty programme paid from fees or a treasury |
| **one-time** | funded **once**; a second funding is refused with `AlreadyFunded` (`0xC00E`), even after the window ends | a parent token spinning a child token off to its holders: "these tokens, over this window" cannot be extended, diluted or topped up |

Everything else is the same, including what a transfer does:

* **History stays with the historical holder.** What a holder earned up to the moment they sell is
  theirs to claim, whoever holds the token afterwards.
* **The future follows the balance.** From then on the buyer, if registered, accrues on the balance
  they now hold.
* **Only registered accounts earn,** and the pool's vault can never register.

For a spin-off the child token must already exist and be a plain token (a child with a Transfer
Hook of its own is refused: its claims would need extra accounts this program does not forward).
This program does not create it. The window is chosen by whoever funds it, once: fund it from an
account you trust to choose it, or revoke the program's upgrade authority.

The CLI runs the two as `holder-rewards` and `holder-rewards-one-time`. This replaces the separate
`loyalty-rewards` and `parent-spin-off` templates.

## How it works

A reward pool is funded with an `amount` to be paid out evenly over `duration` seconds. Every
second's share is split between the **registered** token accounts in proportion to their balance.

The pool keeps one running number, the **index**: reward earned so far *per unit of balance*. It
grows every second by `rate / eligible_supply`. A holder remembers the index at which they were last
settled, so what they have earned is always `balance x (index - index_paid)`. No loop over holders:
a transfer settles the two accounts it touches, in constant time. That is why it is cheap enough to
run inside a swap.

```rust
// src/rule.rs: what a transfer does to the two holders it touches
holder.on_balance_change(&mut stream, balance_before, balance_after)?;
```

## What is in this folder

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Stream`, `Holder`, the index maths. Unit-tested alone, including the loyal-vs-latecomer cases. |
| `src/state.rs` | The global account (the stream) and one record per registered token account. |
| `src/instruction.rs` | `Initialize`, `Register`, `Fund`, `Claim`. |
| `src/processor/` | The instructions and `Execute`, built on [`hook-kit`](../../crates/hook-kit). |
| `src/error.rs` | Error codes from `0xC001`. |
| `tests/holder_rewards.rs` | Runtime tests with exact payouts (ongoing mode). |
| `tests/one_time.rs` | The one-time mode: exact payouts, the single-funding rule, and the guards. |

## The instructions

| Instruction | Who | What |
|---|---|---|
| `Initialize` | the mint's hook authority | picks the mode (ongoing, or one-time) and creates the global, the reward vault and the validation list. The mint must have **no mint authority**, and the reward mint must have no hook. |
| `Register` | anyone, who pays the record's rent | starts counting a token account in the stream. The pool vault is refused. |
| `Fund` | anyone | pays reward tokens into the vault and starts or extends the stream. What the current period has not yet paid is rolled into the new one. In one-time mode only the first funding is accepted. |
| `Claim` | the token account's owner | pays what the account has earned to an account of the reward mint. |

The **reward vault** is a token account at a program-derived address, owned (as a token account) by
the global PDA, so only this program can move rewards out. The reward token can be any plain mint of
Token-2022 or the classic SPL Token program.

## Accounts

Three extra accounts per transfer, all **writable**, so a swap leg is 5 accounts: the global, the
source's record and the destination's record (`["holder", token_account]`), then the hook program
and the validation list. An integrator must name them as allowed writable accounts
(`PrivilegePolicy::allowing_writable`), or the SDK refuses the leg.

## Errors

| Code | Name | When |
|---|---|---|
| `0xC001` | `InvalidInstruction` | malformed instruction |
| `0xC002` | `InvalidGlobal` | wrong or malformed global |
| `0xC003` | `InvalidRecord` | wrong or malformed holder record |
| `0xC004` | `ExcludedAccount` | the pool vault tried to register |
| `0xC005` | `NotRegistered` | claiming with no record |
| `0xC006` | `ZeroAmount` | funding nothing (or less than one unit per second) |
| `0xC007` | `InvalidDuration` | zero, or longer than about ten years |
| `0xC008` | `MathOverflow` | a fixed-point calculation overflowed |
| `0xC009` | `RewardMintHasHook` | the reward mint has a hook of its own |
| `0xC00A` | `RewardAccountMismatch` | wrong vault, mint, token program or payout account |
| `0xC00B` | `WrongOwner` | the signer does not own the token account |
| `0xC00C` | `NothingToClaim` | no earnings yet |
| `0xC00D` | `PoolVaultMismatch` | the pool vault is for another mint |
| `0xC00E` | `AlreadyFunded` | a one-time allocation was already funded |
| `0x8001..` | `hook_kit::KitError` | shared checks: wrong authority, mint authority not revoked, direct `Execute`, already registered, ... |

## Run it

```bash
cargo test -p holder-rewards-hook                       # native
cargo build-sbf --manifest-path templates/holder-rewards/Cargo.toml --sbf-out-dir target/integration-sbf
SBF_OUT_DIR=target/integration-sbf cargo test -p holder-rewards-hook   # the real SBF binary
```

Through Raydium: `cpmm_with_the_holder_rewards_template` and `clmm_with_the_holder_rewards_template`,
and `cpmm_with_the_holder_rewards_one_time_mode` and `clmm_with_the_holder_rewards_one_time_mode`, in
`tests/program-test/tests/local_flows.rs`. They check that swaps settle the registered holder, then
fund the stream, let time pass, claim, and confirm the reward arrived in the holder's quote account;
in one-time mode they also try a second funding and require the hook's own refusal.

## Honest limits

* **Registration is explicit.** An account earns nothing until someone calls `Register` (and pays
  about 0.0015 SOL of rent). A wallet UI would add `Register` to a buyer's first transaction. We did
  **not** build lazy creation inside the swap: it would need a funded rent vault and the system
  program on every transfer, two more accounts per leg, and a policy for when the vault runs dry.
* **The pool earns nothing; other pools and contracts might.** Only the configured pool vault is
  refused. A second pool's vault, or any program-owned account, can register and collect rewards
  that belong to holders.
* **Burning is invisible to a hook.** A holder's earnings are capped by their actual balance when
  they settle, and the eligible supply is corrected at their next transfer or claim, but until then
  burned tokens dilute everyone else a little. That is why the mint authority must be revoked:
  minting would be invisible too.
* **Time with nobody registered pays nobody.** While the eligible supply is zero, that time's
  rewards stay in the vault.
* **Contention.** The global is a writable account in every transfer where either side is
  registered, so those transfers serialise on it.
* **Rounding goes down.** The vault never owes more than it was funded with, and a few units of
  dust can stay in it.
* **Fixed supply only.** The rule assumes balances change only through transfers.
* **The program's upgrade authority can replace this rule.** Disclose it or revoke it.
