//! CLMM limit orders with the hook live: open, increase, decrease, settle and close.
//!
//! Two orders on the pool the liquidity checks use: one that sells the hooked token (`zero_for_one`, its
//! input is the hooked token, its output the quote token) and one that buys it (input the quote token,
//! output the hooked token). Between them every hooked-token transfer a limit order can make is
//! exercised: a deposit, a top-up, a refund, and a payout of filled output. Swaps that move the price
//! across each order's tick fill them; the swaps themselves run the hook as always.
//!
//! The hook-aware instructions (`*_limit_order_v2`) take the order's input token's slice, then its output
//! token's, as the last remaining accounts; a token an instruction does not move has no slice.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Signer};
use transfer_hook_sdk::{frame_clmm_limit_order_v2, ClmmLimitOrderOp, LegRole, SplTransferLeg};

use super::{
    clmm::ClmmSwaps,
    clmm_liquidity::{balances, run_hooked_runs},
    recorder::Recorder,
    support::*,
    swaps::{describe, expected_runs, one_leg, SwapBuilder, SwapKit},
};
use crate::{
    chain::{Chain, DriverError, Result},
    clmm::{
        limit_order_amounts, limit_order_nonce_count, pool_tick_current, Clmm, ClmmLimitOrder,
        ClmmPool, TICK_SPACING,
    },
    hooks::{Direction, RejectionPlan},
};

/// What the first order deposits, adds, and takes back before anything fills.
const ORDER_AMOUNT: u64 = 50;
const ORDER_TOP_UP: u64 = 20;
const ORDER_PARTIAL_CANCEL: u64 = 10;
/// Size of the swaps that push the price across an order, and how many it may take.
const FILL_SWAP_AMOUNT: u64 = 90;
const FILL_SWAPS_AT_MOST: u64 = 8;

/// Which hook-run counts an operation moving `mints` must show: each hook once per moved leg of its mint.
fn runs_for(kit: &SwapKit<'_>, mints: &[Pubkey]) -> Vec<(Pubkey, usize)> {
    let mut runs: Vec<(Pubkey, usize)> = Vec::new();
    for entry in &kit.hooks {
        let count = mints
            .iter()
            .filter(|mint| **mint == entry.ctx.hooked_mint)
            .count();
        let program = entry.hook.program_id();
        match runs.iter_mut().find(|(p, _)| *p == program) {
            Some((_, total)) => *total += count,
            None => runs.push((program, count)),
        }
    }
    runs
}

/// One side of an order: the vault, the mint and the user's token account.
struct Side {
    vault: Pubkey,
    mint: Pubkey,
    account: Pubkey,
}

struct Order {
    state: ClmmLimitOrder,
    input: Side,
    output: Side,
}

impl Order {
    #[allow(clippy::too_many_arguments)]
    fn new(
        clmm: &Clmm,
        pool: &ClmmPool,
        provider: [Pubkey; 2],
        owner: &Pubkey,
        nonce_index: u8,
        orders_so_far: u64,
        zero_for_one: bool,
        tick_index: i32,
    ) -> Self {
        let state = clmm.limit_order(owner, nonce_index, orders_so_far, zero_for_one, tick_index);
        let (input, output) = if zero_for_one {
            (
                Side {
                    vault: pool.vault_0,
                    mint: pool.mint_0,
                    account: provider[0],
                },
                Side {
                    vault: pool.vault_1,
                    mint: pool.mint_1,
                    account: provider[1],
                },
            )
        } else {
            (
                Side {
                    vault: pool.vault_1,
                    mint: pool.mint_1,
                    account: provider[1],
                },
                Side {
                    vault: pool.vault_0,
                    mint: pool.mint_0,
                    account: provider[0],
                },
            )
        };
        Self {
            state,
            input,
            output,
        }
    }

    /// The input token going into the pool (open, increase).
    fn deposit(&self, owner: Pubkey, amount: u64) -> SplTransferLeg {
        SplTransferLeg {
            source: self.input.account,
            mint: self.input.mint,
            destination: self.input.vault,
            authority: owner,
            amount,
        }
    }

    /// The input token coming back (decrease).
    fn refund(&self, pool_state: Pubkey, amount: u64) -> SplTransferLeg {
        SplTransferLeg {
            source: self.input.vault,
            mint: self.input.mint,
            destination: self.input.account,
            authority: pool_state,
            amount,
        }
    }

    /// The output token paid to the owner (decrease, settle).
    fn payout(&self, pool_state: Pubkey, amount: u64) -> SplTransferLeg {
        SplTransferLeg {
            source: self.output.vault,
            mint: self.output.mint,
            destination: self.output.account,
            authority: pool_state,
            amount,
        }
    }
}

/// Frame `instruction` as `op`'s hook-aware version.
async fn frame<C: Chain>(
    chain: &C,
    kit: &SwapKit<'_>,
    op: ClmmLimitOrderOp,
    instruction: &mut Instruction,
    input: Option<SplTransferLeg>,
    output: Option<SplTransferLeg>,
) -> Result<()> {
    let input = match input {
        Some(leg) => Some(one_leg(chain, kit, LegRole::Input, leg).await?),
        None => None,
    };
    let output = match output {
        Some(leg) => Some(one_leg(chain, kit, LegRole::Output, leg).await?),
        None => None,
    };
    frame_clmm_limit_order_v2(op, instruction, input.as_ref(), output.as_ref())
        .map(|_| ())
        .map_err(|e| DriverError::new(format!("framing {} failed: {e:?}", op.name())))
}

async fn tick_current<C: Chain>(chain: &mut C, pool: &ClmmPool) -> Result<i32> {
    let data = raw_data(chain, &pool.pool_state).await?;
    pool_tick_current(&data)
        .ok_or_else(|| DriverError::new("the pool state is too short to hold a tick"))
}

async fn order_amounts<C: Chain>(chain: &mut C, order: &Order) -> Result<(u64, u64)> {
    let data = raw_data(chain, &order.state.order).await?;
    limit_order_amounts(&data)
        .ok_or_else(|| DriverError::new("the limit order account is too short"))
}

async fn account_exists<C: Chain>(chain: &mut C, key: &Pubkey) -> Result<bool> {
    Ok(chain.account(key).await?.is_some())
}

/// Swap across `target` until the pool's tick has moved past it, with a bounded number of swaps.
async fn push_price<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    swaps: &ClmmSwaps<'_>,
    upwards: bool,
    target: i32,
    label: &str,
) -> Result<()> {
    for repeat in 0..FILL_SWAPS_AT_MOST {
        let now = tick_current(chain, &swaps.pool).await?;
        let crossed = if upwards { now > target } else { now < target };
        if crossed {
            return Ok(());
        }
        // Buying the hooked token (the quote token in) moves the price up; selling it moves it down. A
        // different amount each time: an identical transaction in the same block would be a duplicate.
        let instruction = swaps
            .build(chain, !upwards, FILL_SWAP_AMOUNT + repeat, 1)
            .await?;
        let (signature, detail) =
            run_hooked_runs(chain, label, instruction, &[], expected_runs(swaps.kit)).await?;
        rec.push(
            &format!("swap that fills a limit order ({label})"),
            Some(signature),
            detail,
        );
    }
    let now = tick_current(chain, &swaps.pool).await?;
    require(
        if upwards { now > target } else { now < target },
        format!(
            "the price did not cross tick {target} (now {now}) within {FILL_SWAPS_AT_MOST} swaps"
        ),
    )
}

#[allow(clippy::too_many_lines)]
pub(super) async fn limit_order_checks<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    clmm: &Clmm,
    swaps: &ClmmSwaps<'_>,
    provider: [Pubkey; 2],
) -> Result<()> {
    let pool = swaps.pool;
    let kit = swaps.kit;
    let payer = chain.payer().pubkey();

    // Ticks either side of the current price, on the tick arrays the position already created.
    let spacing = i32::from(TICK_SPACING);
    let now = tick_current(chain, &pool).await?;
    require(
        (-280..=280).contains(&now),
        format!("the price is too close to the end of the position's range for limit orders: tick {now}"),
    )?;
    let base = now.div_euclid(spacing) * spacing;
    let (tick_sell, tick_buy) = (base + spacing, base - spacing);

    let nonce_a = clmm.limit_order(&payer, 0, 0, true, tick_sell);
    let count_a = match chain.account(&nonce_a.nonce).await? {
        Some(account) => limit_order_nonce_count(&account.data).unwrap_or(0),
        None => 0,
    };
    let sell = Order::new(clmm, &pool, provider, &payer, 0, count_a, true, tick_sell);
    let nonce_b = clmm.limit_order(&payer, 1, 0, false, tick_buy);
    let count_b = match chain.account(&nonce_b.nonce).await? {
        Some(account) => limit_order_nonce_count(&account.data).unwrap_or(0),
        None => 0,
    };
    let buy = Order::new(clmm, &pool, provider, &payer, 1, count_b, false, tick_buy);

    // 1. Open the order that sells the hooked token: its deposit runs the hook.
    let before = balances(chain, &provider).await?;
    let mut open = clmm.open_limit_order_instruction(
        &pool,
        &sell.state,
        ORDER_AMOUNT,
        &sell.input.account,
        &sell.output.account,
    );
    frame(
        chain,
        kit,
        ClmmLimitOrderOp::Open,
        &mut open,
        Some(sell.deposit(payer, ORDER_AMOUNT)),
        None,
    )
    .await?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "open limit order",
        open,
        &[],
        runs_for(kit, &[sell.input.mint]),
    )
    .await?;
    let after = balances(chain, &provider).await?;
    require(
        before.0 - after.0 == ORDER_AMOUNT && account_exists(chain, &sell.state.order).await?,
        format!("open: the hooked token should have left the account: {before:?} -> {after:?}"),
    )?;
    rec.push(
        "hooked limit order opening (open_limit_order_v2)",
        Some(signature),
        format!("sells the hooked token at tick {tick_sell}; {detail}"),
    );

    // 2. Add to it.
    let before = balances(chain, &provider).await?;
    let mut increase = clmm.increase_limit_order_instruction(
        &pool,
        &sell.state,
        ORDER_TOP_UP,
        &sell.input.account,
    );
    frame(
        chain,
        kit,
        ClmmLimitOrderOp::Increase,
        &mut increase,
        Some(sell.deposit(payer, ORDER_TOP_UP)),
        None,
    )
    .await?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "increase limit order",
        increase,
        &[],
        runs_for(kit, &[sell.input.mint]),
    )
    .await?;
    let after = balances(chain, &provider).await?;
    require(
        before.0 - after.0 == ORDER_TOP_UP,
        format!("increase: {before:?} -> {after:?}"),
    )?;
    rec.push(
        "hooked limit order increase (increase_limit_order_v2)",
        Some(signature),
        detail,
    );

    // 3. Take some of it back: nothing has filled, so only the input token's refund moves.
    let before = balances(chain, &provider).await?;
    let mut decrease = clmm.decrease_limit_order_instruction(
        &pool,
        &sell.state,
        ORDER_PARTIAL_CANCEL,
        0,
        &sell.input.account,
        &sell.output.account,
    );
    frame(
        chain,
        kit,
        ClmmLimitOrderOp::Decrease,
        &mut decrease,
        Some(sell.refund(pool.pool_state, ORDER_PARTIAL_CANCEL)),
        Some(sell.payout(pool.pool_state, 1)),
    )
    .await?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "decrease limit order",
        decrease,
        &[],
        runs_for(kit, &[sell.input.mint]),
    )
    .await?;
    let after = balances(chain, &provider).await?;
    require(
        // A transfer fee, if the mints have one, is withheld from what the owner receives.
        after.0 > before.0
            && after.0 - before.0 <= ORDER_PARTIAL_CANCEL
            && after.0 - before.0 + ORDER_PARTIAL_CANCEL / 5 >= ORDER_PARTIAL_CANCEL,
        format!("decrease: {before:?} -> {after:?}"),
    )?;
    rec.push(
        "hooked limit order decrease (decrease_limit_order_v2)",
        Some(signature),
        detail,
    );

    // 4. A deposit the hook refuses (if it declares an amount rule): nothing may move.
    let primary = kit.primary();
    let refusal = primary.hook.refusals().into_iter().find(|r| {
        r.direction == Direction::HookedIn && matches!(r.plan, RejectionPlan::OverAmount { .. })
    });
    if let Some(refusal) = refusal {
        let RejectionPlan::OverAmount { amount_in } = refusal.plan else {
            unreachable!("matched above")
        };
        let probe = Order::new(clmm, &pool, provider, &payer, 2, 0, true, tick_sell);
        let mut over = clmm.open_limit_order_instruction(
            &pool,
            &probe.state,
            amount_in,
            &probe.input.account,
            &probe.output.account,
        );
        frame(
            chain,
            kit,
            ClmmLimitOrderOp::Open,
            &mut over,
            Some(probe.deposit(payer, amount_in)),
            None,
        )
        .await?;
        let transaction = with_budget(vec![over]);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            !sim.succeeded && sim.program_failed_with(&primary.hook.program_id(), refusal.code),
            format!(
                "an over-limit limit order must be refused by the hook with {:#x}: {:?}",
                refusal.code, sim.logs
            ),
        )?;
        require(
            balances(chain, &provider).await? == after
                && !account_exists(chain, &probe.state.order).await?,
            "balances moved or an order was created after a refused limit order",
        )?;
        rec.push(
            "hook refused limit order (over-amount), nothing changed",
            None,
            format!("rejected with {:#x}; {}", refusal.code, describe(&sim)),
        );
    }

    // 5. The order that buys the hooked token: its deposit is the quote token, and what fills it is
    // paid in the hooked token.
    let before = balances(chain, &provider).await?;
    let mut open = clmm.open_limit_order_instruction(
        &pool,
        &buy.state,
        ORDER_AMOUNT,
        &buy.input.account,
        &buy.output.account,
    );
    frame(
        chain,
        kit,
        ClmmLimitOrderOp::Open,
        &mut open,
        Some(buy.deposit(payer, ORDER_AMOUNT)),
        None,
    )
    .await?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "open limit order (buy)",
        open,
        &[],
        runs_for(kit, &[buy.input.mint]),
    )
    .await?;
    let after = balances(chain, &provider).await?;
    require(
        before.1 - after.1 == ORDER_AMOUNT,
        format!("open (buy): {before:?} -> {after:?}"),
    )?;
    rec.push(
        "limit order opening for the quote token (open_limit_order_v2)",
        Some(signature),
        format!("buys the hooked token at tick {tick_buy}; {detail}"),
    );

    // 6. Fill both with swaps: up through the selling order's tick, then down through the buying one's.
    push_price(chain, rec, swaps, true, tick_sell, "hooked token out").await?;
    push_price(chain, rec, swaps, false, tick_buy, "hooked token in").await?;

    // 7. Settle the buying order: what filled is paid in the hooked token, so the hook runs on the payout.
    let before = balances(chain, &provider).await?;
    let mut settle =
        clmm.settle_limit_order_instruction(&pool, &buy.state, &payer, &buy.output.account);
    frame(
        chain,
        kit,
        ClmmLimitOrderOp::Settle,
        &mut settle,
        None,
        Some(buy.payout(pool.pool_state, 1)),
    )
    .await?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "settle limit order",
        settle,
        &[],
        runs_for(kit, &[buy.output.mint]),
    )
    .await?;
    let after = balances(chain, &provider).await?;
    let (total, filled) = order_amounts(chain, &buy).await?;
    require(
        after.0 > before.0 && filled > 0,
        format!("settle: the hooked token should have been paid out: {before:?} -> {after:?}, filled {filled}/{total}"),
    )?;
    rec.push(
        "hooked limit order settlement (settle_limit_order_v2)",
        Some(signature),
        format!(
            "{filled} of {total} filled, {} of the hooked token paid out; {detail}",
            after.0 - before.0
        ),
    );

    // 8. Cancel what is left of each order. A decrease also settles what has filled, so for the selling
    // order (paid in the quote token) the payout and the refund of the hooked token may both move.
    for (order, label) in [(&sell, "selling order"), (&buy, "buying order")] {
        let before = balances(chain, &provider).await?;
        let mut decrease = clmm.decrease_limit_order_instruction(
            &pool,
            &order.state,
            u64::MAX,
            0,
            &order.input.account,
            &order.output.account,
        );
        frame(
            chain,
            kit,
            ClmmLimitOrderOp::Decrease,
            &mut decrease,
            Some(order.refund(pool.pool_state, 1)),
            Some(order.payout(pool.pool_state, 1)),
        )
        .await?;
        // How many transfers the instruction makes depends on what has filled, so the runs are checked
        // against what actually moved rather than fixed in advance.
        let transaction = with_budget(vec![decrease]);
        let sim = chain.simulate(&transaction, &[]).await?;
        require(
            sim.succeeded,
            format!(
                "cancel {label}: simulation failed: {:?} {:?}",
                sim.error, sim.logs
            ),
        )?;
        let sent = chain
            .send(&transaction, &[])
            .await
            .map_err(|e| DriverError::new(format!("cancel {label}: failed: {e}")))?;
        let after = balances(chain, &provider).await?;
        let moved = [after.0 != before.0, after.1 != before.1];
        let mints = [swaps.pool.mint_0, swaps.pool.mint_1];
        let moved_mints: Vec<Pubkey> = mints
            .iter()
            .zip(moved)
            .filter(|(_, moved)| *moved)
            .map(|(mint, _)| *mint)
            .collect();
        for (program, expected) in runs_for(kit, &moved_mints) {
            require(
                sim.invocations_of(&program) == expected,
                format!(
                    "cancel {label}: hook {program} must run once per moved hooked token ({expected}), ran {}",
                    sim.invocations_of(&program)
                ),
            )?;
        }
        let (total, filled) = order_amounts(chain, order).await?;
        require(
            total == filled,
            format!("cancel {label}: {filled} of {total} filled, nothing should be left"),
        )?;
        rec.push(
            &format!("hooked limit order cancellation ({label}, decrease_limit_order_v2)"),
            Some(sent.signature),
            format!("balances {before:?} -> {after:?}; {}", describe(&sim)),
        );
    }

    // 9. Close both, which returns their rent and moves no tokens.
    for order in [&sell, &buy] {
        send_step(
            chain,
            rec,
            "close a limit order",
            vec![clmm.close_limit_order_instruction(&order.state)],
            &[],
        )
        .await?;
        require(
            !account_exists(chain, &order.state.order).await?,
            "a closed limit order account must be gone",
        )?;
    }
    Ok(())
}
