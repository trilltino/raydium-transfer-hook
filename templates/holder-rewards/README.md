# Holder Rewards

Holders earn a reward stream in another token in proportion to **balance x time held**, settled
inside the transfer, with no loop over holders.

**The rule is [`src/rule.rs`](src/rule.rs). It is pure: no accounts, no Solana types.** The rest of
the folder is plumbing.

**Status:** reference implementation, not audited. Read LIMITATIONS and TRUST before using it.

## WHAT

A reward pool is funded with an `amount` to be paid out evenly over `duration` seconds. Each
second's share is split between the **registered** token accounts in proportion to their balance.

The pool keeps one running number, the **global reward index**: reward earned so far *per unit of
balance*. It grows every second by `rate / eligible_supply`. A holder remembers the index at which
they were last settled, so what they have earned is always `balance x (index - index_paid)`. A
transfer settles the two accounts it touches in constant time. That is why it is cheap enough to run
inside every transfer, and it is the reusable idea of this template.

```rust
// src/rule.rs: what a transfer does to each holder it touches
holder.on_balance_change(&mut stream, balance_before, balance_after)?;
```

Two modes, chosen at `Initialize`:

| Mode | Funding | Use |
|---|---|---|
| **ongoing** (default) | can be topped up if the new period does not lower the current rate; the unpaid part rolls into the new period | a loyalty programme paid from fees or a treasury |
| **one-time** | funded **once**; a second funding is refused with `AlreadyFunded` (`0xC00E`) | a parent token spinning a child token off to its holders: "these tokens, over this window" cannot be extended or diluted |

## WHY

Paying holders by *balance x time* rewards loyalty: the long-term holder earns more than the
latecomer, and buying just before a payout earns almost nothing. The naive implementation loops over
every holder (impossible on chain); the global index makes it constant time.

## TRIGGER

The hook runs on **every** transfer of the hooked mint. It changes state only when either side has
a registered record: it settles both sides, then moves the eligible supply by the balance change.
A transfer between unregistered accounts changes nothing, but it still carries the global and both
record addresses as **writable** accounts (the validation list declares them so), so it still takes
the write lock on the global; see STATE / COST.

* **History stays with the historical token account** (for ordinary transfers). What an account
  earned up to the moment it sells is its to claim, whoever holds the token afterwards. Rewards
  belong to the token account, not a wallet; see LIMITATIONS for owner changes and closing.
* **The future follows the balance.** From then on the buyer, if registered, accrues on what they
  now hold.
* **Only registered accounts earn.** The pool's vault can never register.

## EXAMPLE

Fund 1,000 reward tokens over 100 seconds (10 per second). A holds 600, B holds 400, both registered.

| Time | Event | Earned so far |
|---|---|---|
| t = 50 | nothing yet | A 300, B 200 |
| t = 50 | A transfers 300 to B | settled first: A 300, B 200; then A holds 300, B 700 |
| t = 100 | the period ends | A 300 + 150 = 450, B 200 + 350 = 550 |

The two add up to the 1,000 funded (up to a few units of rounding dust, which stay in the vault).
A claims 450 whenever they like, even though they sold half their balance at t = 50.

## RULES

* `Initialize` (the mint's hook authority): picks the mode; creates the global, the **reward vault**
  (a token account at a program-derived address, owned by the global PDA so only this program can
  move rewards out) and the validation list. The hooked mint must have **no mint authority**, and the
  reward mint must have no Transfer Hook of its own. The hooked mint must not carry a transfer fee or
  confidential balance extension. The reward token can be a classic SPL Token mint or a Token-2022
  mint with descriptive extensions only. Transfer fees, permanent delegates and other extensions
  that affect vault safety or payout amounts are refused.
* `Register` (anyone, who pays the record's rent): starts counting a token account. The pool vault is
  refused.
* `Fund` (anyone): pays reward tokens into the vault and starts or extends the stream. An ongoing
  top-up during a running period must keep or raise the current reward rate. Rejects a zero amount,
  less than one unit per second, a zero duration, or one longer than `MAX_DURATION` (315,360,000 s,
  about ten years).
* `Claim` (the token account's owner): pays what the account has earned to an account of the reward
  mint.
* `Reconcile` (anyone): corrects a record whose balance fell without a transfer, a burn or a closed
  token account, so it stops diluting everyone else. It only ever lowers a stale count; accrual on
  the vanished tokens since their last settlement is forfeited (it stays in the vault).
* Rounding goes down: the vault never owes more than it was funded with.

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
| `0xC00F` | `FundingLowersRate` | an ongoing top-up would slow a running stream |
| `0xC010` | `UnsupportedMintExtension` | a mint extension breaks accounting or vault safety |
| `0xC011` | `NothingToReconcile` | the token account still holds what its record counts |
| `0x8001..` | `hook_kit::KitError` | shared checks: wrong authority, mint authority not revoked, direct `Execute`, already initialised, ... |

## LIMITATIONS

* **Every transfer of the mint serialises on the global.** It is writable in every transfer, whether
  or not anyone is registered, so all transfers of this mint, in every pool and wallet, contend for
  one account within a block. This is the main throughput cost of the template.
* **Registration is explicit.** An account earns nothing until someone calls `Register` (about
  0.0012 SOL of rent for its record at the time of writing). A wallet would add `Register` to a
  buyer's first transaction. Lazy creation inside the transfer was not built: it needs a funded
  rent vault and the system program on every transfer, and a policy for when the vault runs dry.
* **Only the configured pool vault is excluded.** A second pool's vault, or any program-owned
  account, can register and collect rewards that belong to holders.
* **Burns and closed accounts are invisible to a hook.** A holder's earnings are capped by their real
  balance when they settle, and the eligible supply is corrected at their next transfer or claim, or
  when **anyone** calls `Reconcile`, which also works after the token account was closed. Until then
  the burned balance dilutes everyone else, so integrators should reconcile accounts they see burn
  or close. Minting is invisible too, which is why the mint authority must be revoked: the rule
  assumes a fixed supply.
* **Claim before closing.** `Claim` needs a live token account whose owner signs. Earnings left in a
  record when its token account is closed cannot be claimed (they stay in the vault); there is
  nothing for `Reconcile` to release either if the balance was already zero.
* **Rewards follow the token account, not the wallet.** Changing the account's owner
  (`SetAuthority`) does not invoke the hook: the new owner can claim everything the account earned,
  including before they owned it. Claim before handing an account over.
* **Time with nobody registered pays nobody.** While the eligible supply is zero, that time's
  rewards stay in the vault.
* **A one-time allocation's window is chosen by whoever funds it first.** Fund it from an account
  you trust to choose it, or revoke the program's upgrade authority.
* **A one-time child token must already exist and be a plain token** (a child with a Transfer Hook
  of its own is refused: its claims would need extra accounts this program does not forward). This
  program does not create it.
* The index uses a scale of 10^18. Each update rounds down by less than one index unit; at the
  largest possible eligible supply this can leave up to 18 raw reward units per update in the
  vault. Very small rates against an extreme supply can still round to zero on every update.

## TRUST

* **Config authority:** `Initialize` is signed by the mint's live Transfer Hook authority, once.
  There is no update instruction. Funding is open to anyone: in ongoing mode anyone can top up
  without reducing the current rate; in
  one-time mode the *first* funder fixes the amount and the window.
* **Program upgrade authority:** can replace this rule, including how the vault pays out, for every
  mint that uses the program. Disclose it or revoke it
  (`solana program set-upgrade-authority <ID> --final`).
* **Mint hook selection:** the mint's Transfer Hook authority can re-point the mint at a different
  hook and stop accounting. Revoke the extension's authority to make the programme durable.
* The reward vault is owned by a program-derived account, so only this program can pay out of it.

## STATE / COST

Three extra accounts per transfer, **all writable**: the global (`["rewards", mint]`), the source's
record and the destination's record (`["holder", token_account]`); plus the hook program and the
validation list, so a transfer leg carries 5 hook accounts.

**Contention.** The runtime takes write locks from the transaction's account list, not from what the
program ends up writing. The global is declared writable, so **every** transfer of the mint locks it,
including transfers where neither side is registered and the hook writes nothing; all of them
serialise on that one account. The source and destination records are writable too, but they are
per token account, so they only contend between transfers touching the same account. Keep this in
mind for a busy mint: the global is a throughput ceiling that spreading trades over several pools
does not remove.

Each registered account costs a 105-byte record (it stores its mint so `Reconcile` works after the
token account is closed); the global is 186 bytes; setup also creates the
reward vault (165 bytes) and the 121-byte validation list (about 0.0012 SOL per record and about
0.0044 SOL of setup rent at the time of writing; `solana rent <bytes>` gives current figures).
Callers (and tests) must allow the three writable accounts explicitly, or a careful integrator will
refuse the transfer. `Execute` also re-derives the global and both record addresses on every
transfer, before deciding whether anything is registered.

## TESTS

`cargo test` from this directory. Rule unit tests (`src/rule.rs`) cover the index maths; runtime
tests (`tests/holder_rewards.rs`, `tests/one_time.rs`) run inside real Token-2022 transfers with exact
payouts.

| Behaviour | Test |
|---|---|
| valid, exact payouts | `earnings_follow_balance_and_time_with_exact_payouts`, `rewards_split_in_proportion_to_balance`, `a_lone_holder_earns_the_whole_stream` |
| loyal vs latecomer | `the_loyal_holder_earns_more_than_the_latecomer`, `buying_just_before_the_end_earns_almost_nothing` |
| state update correctness | `a_transfer_settles_both_sides_and_moves_the_eligible_supply`, `history_stays_with_the_seller_and_the_future_follows_the_buyer` (rule and runtime) |
| boundaries | `invalid_funding_is_rejected`, `funding_validates_amount_and_duration_and_rolls_over`, `frequent_updates_lose_nothing_against_a_large_supply`, `a_stranger_cannot_slow_a_running_stream_but_can_still_top_it_up`, `the_arithmetic_cannot_overflow_at_the_extremes`, `payouts_never_exceed_the_funded_amount_even_with_awkward_numbers` |
| rejections, exact codes | `a_reward_mint_that_can_pay_out_less_or_be_drained_is_refused`, `a_hooked_mint_with_a_transfer_fee_is_refused`, `a_stranger_cannot_stretch_a_running_stream_with_a_token_top_up`, `claims_are_guarded`, `the_allocation_can_only_be_funded_once`, `a_child_token_with_a_hook_of_its_own_is_refused`, `initialize_creates_the_vault_and_validates_its_inputs` |
| malformed state | `global_and_record_round_trip_and_reject_bad_shapes`, `instructions_round_trip` |
| unauthorized | `only_the_mints_hook_authority_can_initialize`, `claims_are_guarded` (`WrongOwner`) |
| irrelevant path | `an_unregistered_account_earns_nothing`, `nothing_is_paid_to_a_holder_who_never_registered_or_to_the_pool`, `registering_counts_a_balance_once_and_never_the_pool` |
| burn, close, owner change | `a_burned_balance_stops_earning_at_the_next_settlement`, `reconciling_a_burn_releases_the_supply_and_keeps_what_was_earned`, `reconciling_a_closed_account_forfeits_only_unsettled_accrual_on_vanished_tokens` (rule); `burning_and_closing_leaves_the_balance_counted_until_anyone_reconciles`, `reconcile_corrects_a_partial_burn_and_keeps_the_holders_earnings`, `reconcile_refuses_healthy_unregistered_and_mismatched_accounts`, `earnings_left_in_a_closed_account_cannot_be_claimed`, `changing_the_account_owner_hands_its_unclaimed_history_to_the_new_owner` (runtime) |
| direct `Execute` call | `a_direct_execute_call_is_refused` |

```sh
cargo test --locked                            # in this directory
cargo build-sbf --sbf-out-dir target/deploy && SBF_OUT_DIR=$PWD/target/deploy cargo test --locked   # real SBF binary
```

| Path | Role |
|---|---|
| `src/rule.rs` | **The rule**: `Stream`, `Holder`, the index maths |
| `src/state.rs` | the global account and one record per registered token account |
| `src/instruction.rs`, `src/processor/` | `Initialize`, `Register`, `Fund`, `Claim`, `Reconcile` and `Execute`, on [`hook-kit`](../../hook-kit) |
| `src/error.rs` | error codes from `0xC001` |

## DEPLOY / INITIALIZE

**Evidence so far:** in-process and SBF in-process tests (CI runs both). This template ships no
`examples/devnet.rs`, so this repository has not initialised or exercised it on devnet.

1. **Build and deploy.** `scripts/deploy.sh templates/holder-rewards` builds with `cargo build-sbf`
   and deploys to devnet, then stops: it does not create a mint or initialise anything.
2. **Create the mints and the pool vault.** A Token-2022 mint whose Transfer Hook extension points
   at the program id, with its whole supply minted and then its **mint authority revoked**, and no
   transfer-fee or confidential extensions. The pool vault (a token account of that mint). A reward
   mint: classic SPL Token, or Token-2022 with descriptive extensions only.
3. **Initialise.** Send `Initialize` (builders: `instruction::initialize` for an ongoing programme,
   `instruction::initialize_one_time` for a spin-off). Accounts, in order: payer (signer, writable),
   the mint's Transfer Hook authority (signer), mint, pool vault, reward mint, global
   `["rewards", mint]` (writable), reward vault `["reward-vault", mint]` (writable), validation list
   `["extra-account-metas", mint]` (writable), the reward mint's token program, system program. Only
   the mint's live Transfer Hook authority can sign it, and only once.
4. **What it creates.** The global (186 bytes, rate 0 until funded), the reward vault (a token
   account of the reward mint owned by the global PDA), and the validation list (121 bytes, equal to
   `state::VALIDATION_LIST`).
5. **Then.** Holders `Register` (anyone can pay), someone `Fund`s the stream, owners `Claim`, and
   anyone `Reconcile`s an account that burned or closed.
6. **Verify.** Decode the global and check the mint, reward mint, vault, pool vault and mode; check
   the vault's token owner is the global PDA; check the list bytes; register two holders, fund a short
   stream, transfer between them, then claim and check each payout against the expected split.

`tests/holder_rewards.rs` (`init`) and `tests/one_time.rs` (`init`) build exactly this arrangement
in-process.
