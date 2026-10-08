//! CLMM reward emissions whose reward mint has a Transfer Hook.
//!
//! The hooked token of the flow is also the pool's reward token. Four instructions move it:
//!
//! 1. `initialize_reward_v2` funds a reward period from the funder's account into the reward vault;
//! 2. `decrease_liquidity_v4` pays the position's accrued rewards to its owner (alongside the two pool
//!    tokens' own transfers, each with its own hook slice);
//! 3. `set_reward_params_v2` extends the period and tops the vault up;
//! 4. `collect_remaining_rewards_v2` returns to the funder what was never emitted.
//!
//! A reward period lasts at least seven days, so everything after the funding needs the cluster clock to
//! move ([`Chain::can_warp`]). On a live cluster only the funding runs, and the flow says so.
//! While a reward is initialised, every `decrease_liquidity` must pass its reward group, so the flow's
//! later position removal uses [`decrease_instruction`] too.

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Signer};
use transfer_hook_sdk::{
    frame_clmm_decrease_with_rewards_v4, frame_clmm_reward_v2, ClmmRewardOp, LegHook, LegRole,
    SplTransferLeg,
};

use super::{
    clmm::ClmmSwaps,
    clmm_liquidity::{balances, run_hooked_runs, withdrawal_legs},
    recorder::Recorder,
    support::*,
    swaps::{describe, one_leg, pair_legs, SwapKit},
};
use crate::{
    chain::{Chain, DriverError, Result},
    clmm::{Clmm, ClmmPool, ClmmPosition},
};

/// What one reward period pays out in total, in the hooked token's base units. Well inside what every hook
/// in this repository allows per transfer.
const PERIOD_REWARD: u64 = 60;
const PERIOD_SECONDS: u64 = 7 * 24 * 60 * 60;
/// How long after funding the reward opens.
const OPEN_DELAY: u64 = 30;

/// Where the reward lives once it is initialised.
#[derive(Clone, Copy, Debug)]
pub(super) struct RewardState {
    pub(super) mint: Pubkey,
    pub(super) vault: Pubkey,
    pub(super) end_time: u64,
    pub(super) emissions_per_second_x64: u128,
}

impl RewardState {
    /// `(reward vault, recipient, reward mint)` as `decrease_liquidity` wants it.
    pub(super) fn group(&self, recipient: Pubkey) -> (Pubkey, Pubkey, Pubkey) {
        (self.vault, recipient, self.mint)
    }
}

/// Which hook-run counts an operation moving `mints` must show.
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

fn reward_leg(
    state: &RewardState,
    source: Pubkey,
    destination: Pubkey,
    authority: Pubkey,
    amount: u64,
) -> SplTransferLeg {
    SplTransferLeg {
        source,
        mint: state.mint,
        destination,
        authority,
        amount,
    }
}

/// Build a `decrease_liquidity_v2`-shaped instruction for the position and frame it for the hooks: the
/// plain `decrease_liquidity_v3` framing while no reward is initialised, `decrease_liquidity_v4` with the
/// reward's slice once there is one. `rewards` is the reward's `(vault, recipient)` pair if initialised.
#[allow(clippy::too_many_arguments)]
pub(super) async fn decrease_instruction<C: Chain>(
    chain: &C,
    kit: &SwapKit<'_>,
    clmm: &Clmm,
    pool: &ClmmPool,
    position: &ClmmPosition,
    provider: [Pubkey; 2],
    liquidity: u128,
    reward: Option<(&RewardState, Pubkey)>,
) -> Result<Instruction> {
    let (leg_0, leg_1) = withdrawal_legs(pool, provider, 1);
    let Some((state, recipient)) = reward else {
        let mut instruction = clmm.decrease_liquidity_instruction(
            pool,
            position,
            &provider[0],
            &provider[1],
            liquidity,
            0,
            0,
        );
        let (token_0, token_1) = pair_legs(chain, kit, leg_0, leg_1).await?;
        transfer_hook_sdk::frame_clmm_liquidity_v3(
            transfer_hook_sdk::ClmmLiquidityOp::DecreaseLiquidity,
            &mut instruction,
            &token_0,
            &token_1,
        )
        .map_err(|e| DriverError::new(format!("framing decrease_liquidity failed: {e:?}")))?;
        return Ok(instruction);
    };
    let mut instruction = clmm.decrease_liquidity_with_rewards_instruction(
        pool,
        position,
        &provider[0],
        &provider[1],
        liquidity,
        0,
        0,
        &[state.group(recipient)],
    );
    let (token_0, token_1) = pair_legs(chain, kit, leg_0, leg_1).await?;
    let reward: LegHook = one_leg(
        chain,
        kit,
        LegRole::Other(0),
        reward_leg(state, state.vault, recipient, pool.pool_state, 1),
    )
    .await?;
    frame_clmm_decrease_with_rewards_v4(&mut instruction, &token_0, &token_1, &[&reward], 16)
        .map_err(|e| DriverError::new(format!("framing decrease_liquidity_v4 failed: {e:?}")))?;
    Ok(instruction)
}

/// Fund the first reward period: the hooked token, from the provider's account to a new reward vault.
pub(super) async fn start_rewards<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    clmm: &Clmm,
    swaps: &ClmmSwaps<'_>,
    provider: [Pubkey; 2],
) -> Result<RewardState> {
    let pool = swaps.pool;
    let kit = swaps.kit;
    let payer = chain.payer().pubkey();
    let mint = pool.mint_0;

    if chain.account(&clmm.operation_state()).await?.is_none() {
        send_step(
            chain,
            rec,
            "create the operation account that reward instructions check (admin instruction)",
            vec![clmm.create_operation_account_instruction(&payer)],
            &[],
        )
        .await?;
    }

    let now = u64::try_from(chain_time(chain).await?)
        .map_err(|_| DriverError::new("the cluster clock is before 1970"))?;
    let open_time = now + OPEN_DELAY;
    let end_time = open_time + PERIOD_SECONDS;
    // Emissions that pay `PERIOD_REWARD` over the period (rounded up, so the funding is at least that).
    let emissions_per_second_x64 =
        ((u128::from(PERIOD_REWARD) << 64) / u128::from(PERIOD_SECONDS)) + 1;
    let state = RewardState {
        mint,
        vault: clmm.reward_vault(&pool.pool_state, &mint),
        end_time,
        emissions_per_second_x64,
    };

    let before = balances(chain, &provider).await?;
    let mut initialize = clmm.initialize_reward_instruction(
        &payer,
        &provider[0],
        &pool,
        &mint,
        open_time,
        end_time,
        emissions_per_second_x64,
        Some(clmm.support_mint(&mint)),
    );
    let reward = one_leg(
        chain,
        kit,
        LegRole::Other(0),
        reward_leg(&state, provider[0], state.vault, payer, PERIOD_REWARD),
    )
    .await?;
    frame_clmm_reward_v2(ClmmRewardOp::InitializeReward, &mut initialize, &reward)
        .map_err(|e| DriverError::new(format!("framing initialize_reward failed: {e:?}")))?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "initialize reward",
        initialize,
        &[],
        runs_for(kit, &[mint]),
    )
    .await?;
    let after = balances(chain, &provider).await?;
    let vault = amount_of(chain, &state.vault).await?;
    require(
        before.0 - after.0 >= PERIOD_REWARD && vault > 0,
        format!("initialize reward: funder {before:?} -> {after:?}, vault holds {vault}"),
    )?;
    rec.push(
        "hooked reward funding (initialize_reward_v2)",
        Some(signature),
        format!("the reward vault holds {vault} of the hooked token; {detail}"),
    );
    Ok(state)
}

/// With the clock moved a few days on, pay the position its rewards and extend the period with a top-up.
/// Only on a chain whose clock can be moved; otherwise it records that it did not run.
#[allow(clippy::too_many_arguments)]
pub(super) async fn accrue_pay_and_extend<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    clmm: &Clmm,
    swaps: &ClmmSwaps<'_>,
    provider: [Pubkey; 2],
    position: &ClmmPosition,
    state: &mut RewardState,
) -> Result<()> {
    if !chain.can_warp() {
        rec.push(
            "hooked reward payout and top-up not run",
            None,
            format!(
                "a reward period lasts at least {} days and this cluster's clock cannot be moved",
                PERIOD_SECONDS / 86_400
            ),
        );
        return Ok(());
    }
    let pool = swaps.pool;
    let kit = swaps.kit;
    let payer = chain.payer().pubkey();
    // Rewards are paid to a separate account so that their transfer can be told from the pool tokens'.
    let recipient = swaps.world.trader[0].pubkey();

    // 1. Two days of emissions to the position, then collect them (zero liquidity moves only fees and rewards).
    chain
        .advance_time(2 * 24 * 60 * 60 + OPEN_DELAY + 1)
        .await?;
    let before = (
        balances(chain, &provider).await?,
        amount_of(chain, &recipient).await?,
    );
    let instruction = decrease_instruction(
        chain,
        kit,
        clmm,
        &pool,
        position,
        provider,
        0,
        Some((state, recipient)),
    )
    .await?;
    let transaction = with_budget(vec![instruction]);
    let sim = chain.simulate(&transaction, &[]).await?;
    require(
        sim.succeeded,
        format!(
            "reward payout: simulation failed: {:?} {:?}",
            sim.error, sim.logs
        ),
    )?;
    let sent = chain
        .send(&transaction, &[])
        .await
        .map_err(|e| DriverError::new(format!("reward payout failed: {e}")))?;
    let after = (
        balances(chain, &provider).await?,
        amount_of(chain, &recipient).await?,
    );
    let paid = after.1 - before.1;
    require(
        paid > 0,
        format!("two days of emissions should have paid the position something: {before:?} -> {after:?}"),
    )?;
    // The reward transfer is one hooked run, and so is each pool token that also moved.
    let mut moved = vec![state.mint];
    if after.0 .0 != before.0 .0 {
        moved.push(pool.mint_0);
    }
    if after.0 .1 != before.0 .1 {
        moved.push(pool.mint_1);
    }
    for (program, expected) in runs_for(kit, &moved) {
        require(
            sim.invocations_of(&program) == expected,
            format!(
                "reward payout: hook {program} must run once per moved hooked token ({expected}), ran {}",
                sim.invocations_of(&program)
            ),
        )?;
    }
    rec.push(
        "hooked reward payout (decrease_liquidity_v4)",
        Some(sent.signature),
        format!(
            "the position received {paid} of the hooked token as reward; {}",
            describe(&sim)
        ),
    );

    // 2. Extend the period by another seven days at the same rate: that tops the vault up.
    let now = u64::try_from(chain_time(chain).await?)
        .map_err(|_| DriverError::new("the cluster clock is before 1970"))?;
    let new_end = state.end_time + PERIOD_SECONDS;
    let vault_before = amount_of(chain, &state.vault).await?;
    let funder_before = amount_of(chain, &provider[0]).await?;
    let mut set = clmm.set_reward_params_instruction(
        &payer,
        &pool,
        0,
        state.emissions_per_second_x64,
        now + 60,
        new_end,
        Some((provider[0], state.mint)),
    );
    let top_up = one_leg(
        chain,
        kit,
        LegRole::Other(0),
        reward_leg(state, provider[0], state.vault, payer, PERIOD_REWARD),
    )
    .await?;
    frame_clmm_reward_v2(ClmmRewardOp::SetRewardParams, &mut set, &top_up)
        .map_err(|e| DriverError::new(format!("framing set_reward_params failed: {e:?}")))?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "extend reward",
        set,
        &[],
        runs_for(kit, &[state.mint]),
    )
    .await?;
    let vault_after = amount_of(chain, &state.vault).await?;
    require(
        vault_after > vault_before && amount_of(chain, &provider[0]).await? < funder_before,
        format!("extend reward: the vault should have grown, {vault_before} -> {vault_after}"),
    )?;
    state.end_time = new_end;
    rec.push(
        "hooked reward top-up (set_reward_params_v2)",
        Some(signature),
        format!("the vault grew from {vault_before} to {vault_after}; {detail}"),
    );
    Ok(())
}

/// After the position is gone and the period is over, the funder takes back what was never emitted.
pub(super) async fn collect_remaining<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    clmm: &Clmm,
    swaps: &ClmmSwaps<'_>,
    provider: [Pubkey; 2],
    state: &RewardState,
) -> Result<()> {
    if !chain.can_warp() {
        return Ok(());
    }
    let pool = swaps.pool;
    let kit = swaps.kit;
    let payer = chain.payer().pubkey();
    let now = u64::try_from(chain_time(chain).await?)
        .map_err(|_| DriverError::new("the cluster clock is before 1970"))?;
    chain
        .advance_time(state.end_time.saturating_sub(now) + 24 * 60 * 60)
        .await?;

    let funder_before = amount_of(chain, &provider[0]).await?;
    let vault_before = amount_of(chain, &state.vault).await?;
    let mut collect =
        clmm.collect_remaining_rewards_instruction(&payer, &provider[0], &pool, &state.mint, 0);
    let reward = one_leg(
        chain,
        kit,
        LegRole::Other(0),
        reward_leg(state, state.vault, provider[0], pool.pool_state, 1),
    )
    .await?;
    frame_clmm_reward_v2(ClmmRewardOp::CollectRemainingRewards, &mut collect, &reward).map_err(
        |e| DriverError::new(format!("framing collect_remaining_rewards failed: {e:?}")),
    )?;
    let (signature, detail) = run_hooked_runs(
        chain,
        "collect remaining rewards",
        collect,
        &[],
        runs_for(kit, &[state.mint]),
    )
    .await?;
    let funder_after = amount_of(chain, &provider[0]).await?;
    require(
        funder_after > funder_before,
        format!("collect remaining: the funder should have got the unemitted part back, {funder_before} -> {funder_after} (vault held {vault_before})"),
    )?;
    rec.push(
        "hooked remaining-reward collection (collect_remaining_rewards_v2)",
        Some(signature),
        format!(
            "the funder got back {} of the hooked token; {detail}",
            funder_after - funder_before
        ),
    );
    Ok(())
}
