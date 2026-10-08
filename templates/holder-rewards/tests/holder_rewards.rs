//! The hook inside real Token-2022 transfers: registering, funding, settling on transfers, and
//! claiming with exact payouts; plus setup validation and the claim guards.
//!
//! Account indices: 0 is the pool vault, 1..=3 are holders A, B, C. Runs the hook natively by
//! default; set `SBF_OUT_DIR` to a directory holding `holder_rewards_hook.so` to run the real SBF
//! build.

use holder_rewards_hook::{
    error::HolderRewardsError,
    instruction::{claim, fund, initialize, register},
    process_instruction,
    state::{global_address, record_address, reward_vault_address, Global, Record},
};
use hook_kit::{
    testing::{assert_custom_error, AccountSpec, World},
    validation_list_address, KitError,
};
use solana_program_test::{processor, BanksClientError, ProgramTest};
use solana_sdk::{
    instruction::AccountMeta,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use spl_token_2022::state::Account as TokenAccount;

const POOL: usize = 0;
const A: usize = 1;
const B: usize = 2;
const C: usize = 3;

const T0: i64 = 1_000;

fn program_id() -> Pubkey {
    Pubkey::new_from_array([0xC5; 32])
}

/// A plain reward mint with a funder account and one reward account per holder.
struct Reward {
    mint: Keypair,
    token_program: Pubkey,
    funder_account: Pubkey,
    /// Reward accounts of A, B, C (owned by their owners).
    holder_accounts: [Pubkey; 3],
}

async fn world_with_holders(revoke_mint_authority: bool) -> World {
    let id = program_id();
    let test = ProgramTest::new("holder_rewards_hook", id, processor!(process_instruction));
    let mut world = World::start(
        test,
        id,
        vec![
            AccountSpec::owned_by(Keypair::new(), 1_000_000),
            AccountSpec::owned_by(Keypair::new(), 1_000),
            AccountSpec::owned_by(Keypair::new(), 0),
            AccountSpec::owned_by(Keypair::new(), 0),
        ],
    )
    .await;
    if revoke_mint_authority {
        world.revoke_mint_authority().await;
    }
    world.set_unix_time(T0).await;
    world
}

async fn create_reward(world: &mut World, token_program: Pubkey) -> Reward {
    let token = world
        .create_plain_token(token_program, &[A, B, C], 1_000_000)
        .await;
    Reward {
        mint: token.mint,
        token_program,
        funder_account: token.funder_account,
        holder_accounts: [
            token.holder_accounts[0],
            token.holder_accounts[1],
            token.holder_accounts[2],
        ],
    }
}

fn clone(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).unwrap()
}

async fn reward_balance(world: &mut World, account: Pubkey) -> u64 {
    world.token_balance(account).await
}

async fn global(world: &mut World) -> Global {
    let key = global_address(&world.mint.pubkey(), &program_id()).0;
    Global::decode(&world.data(key).await).unwrap()
}

async fn record(world: &mut World, holder: usize) -> Record {
    let key = record_address(&world.account(holder), &program_id()).0;
    Record::decode(&world.data(key).await).unwrap()
}

/// Initialise the rewards (hook authority = payer).
async fn init(world: &mut World, reward: &Reward) -> Result<(), BanksClientError> {
    let ix = initialize(
        &program_id(),
        &world.payer(),
        &world.payer(),
        &world.mint.pubkey(),
        &world.account(POOL),
        &reward.mint.pubkey(),
        &reward.token_program,
    );
    world.send(&[ix], &[]).await
}

async fn register_holder(world: &mut World, holder: usize) -> Result<(), BanksClientError> {
    let ix = register(
        &program_id(),
        &world.payer(),
        &world.mint.pubkey(),
        &world.account(holder),
    );
    world.send(&[ix], &[]).await
}

async fn fund_rewards(
    world: &mut World,
    reward: &Reward,
    amount: u64,
    duration: u32,
) -> Result<(), BanksClientError> {
    let ix = fund(
        &program_id(),
        &world.payer(),
        &reward.funder_account,
        &world.mint.pubkey(),
        &reward.mint.pubkey(),
        &reward.token_program,
        amount,
        duration,
    );
    world.send(&[ix], &[]).await
}

async fn claim_rewards(
    world: &mut World,
    reward: &Reward,
    holder: usize,
) -> Result<(), BanksClientError> {
    let ix = claim(
        &program_id(),
        &world.owner(holder),
        &world.mint.pubkey(),
        &world.account(holder),
        &reward.holder_accounts[holder - 1],
        &reward.mint.pubkey(),
        &reward.token_program,
    );
    let owner = clone(&world.owners[holder]);
    world.send(&[ix], &[&owner]).await
}

/// The writable extras of a transfer: the global and the two token accounts' records.
fn writable(world: &World, from: usize, to: usize) -> Vec<Pubkey> {
    vec![
        global_address(&world.mint.pubkey(), &program_id()).0,
        record_address(&world.account(from), &program_id()).0,
        record_address(&world.account(to), &program_id()).0,
    ]
}

async fn transfer(
    world: &mut World,
    from: usize,
    to: usize,
    amount: u64,
) -> Result<(), BanksClientError> {
    let extras = writable(world, from, to);
    world.transfer(from, to, amount, &extras).await
}

/// A launched world: rewards initialised, A and B registered, a 10,000 / 100 s stream funded at T0.
async fn running() -> (World, Reward) {
    let mut world = world_with_holders(true).await;
    let reward = create_reward(&mut world, spl_token_2022::id()).await;
    init(&mut world, &reward).await.expect("initialize");
    register_holder(&mut world, A).await.expect("register A");
    register_holder(&mut world, B).await.expect("register B");
    fund_rewards(&mut world, &reward, 10_000, 100)
        .await
        .expect("fund");
    (world, reward)
}

#[tokio::test]
async fn initialize_creates_the_vault_and_validates_its_inputs() {
    // The mint authority must be revoked: minting is invisible to a hook.
    let mut world = world_with_holders(false).await;
    let reward = create_reward(&mut world, spl_token_2022::id()).await;
    assert_custom_error(
        init(&mut world, &reward).await,
        KitError::MintAuthorityNotRevoked.code(),
    );
    world.revoke_mint_authority().await;

    // A reward mint with a TransferHook of its own is refused: here, the hooked mint itself.
    let hooked_as_reward = Reward {
        mint: clone(&world.mint),
        token_program: spl_token_2022::id(),
        funder_account: reward.funder_account,
        holder_accounts: reward.holder_accounts,
    };
    assert_custom_error(
        init(&mut world, &hooked_as_reward).await,
        HolderRewardsError::RewardMintHasHook.code(),
    );

    init(&mut world, &reward).await.expect("initialize");
    assert_custom_error(
        init(&mut world, &reward).await,
        KitError::AlreadyInitialized.code(),
    );

    let stored = global(&mut world).await;
    assert_eq!(stored.mint, world.mint.pubkey());
    assert_eq!(stored.reward_mint, reward.mint.pubkey());
    assert_eq!(stored.pool_vault, world.account(POOL));
    let vault = reward_vault_address(&world.mint.pubkey(), &program_id()).0;
    assert_eq!(stored.reward_vault, vault);
    let vault_data = world.data(vault).await;
    let vault_account = TokenAccount::unpack(&vault_data[..TokenAccount::LEN]).unwrap();
    assert_eq!(vault_account.mint, reward.mint.pubkey());
    assert_eq!(
        vault_account.owner,
        global_address(&world.mint.pubkey(), &program_id()).0,
        "only the program, through the global PDA, can move rewards out"
    );
}

#[tokio::test]
async fn registering_counts_a_balance_once_and_never_the_pool() {
    let mut world = world_with_holders(true).await;
    let reward = create_reward(&mut world, spl_token_2022::id()).await;
    init(&mut world, &reward).await.expect("initialize");

    register_holder(&mut world, A).await.expect("register A");
    assert_eq!(global(&mut world).await.stream.eligible_supply, 1_000);
    assert_eq!(record(&mut world, A).await.holder.checkpoint, 1_000);
    assert_custom_error(
        register_holder(&mut world, A).await,
        KitError::AlreadyInitialized.code(),
    );
    assert_custom_error(
        register_holder(&mut world, POOL).await,
        HolderRewardsError::ExcludedAccount.code(),
    );
    assert_eq!(global(&mut world).await.stream.eligible_supply, 1_000);
}

#[tokio::test]
async fn earnings_follow_balance_and_time_with_exact_payouts() {
    let (mut world, reward) = running().await;
    let vault = reward_vault_address(&world.mint.pubkey(), &program_id()).0;
    assert_eq!(reward_balance(&mut world, vault).await, 10_000);

    // Rate 100/s over [1000, 1100]. A alone holds 1000 until t = 1050, then sends 400 to B.
    world.set_unix_time(T0 + 50).await;
    transfer(&mut world, A, B, 400).await.expect("A to B");
    assert_eq!(record(&mut world, A).await.holder.earned, 5_000);
    assert_eq!(record(&mut world, B).await.holder.earned, 0);
    assert_eq!(global(&mut world).await.stream.eligible_supply, 1_000);

    // Second half: A holds 600 of 1000, B 400.
    world.set_unix_time(T0 + 100).await;
    claim_rewards(&mut world, &reward, A)
        .await
        .expect("claim A");
    claim_rewards(&mut world, &reward, B)
        .await
        .expect("claim B");
    assert_eq!(
        reward_balance(&mut world, reward.holder_accounts[0]).await,
        5_000 + 3_000
    );
    assert_eq!(
        reward_balance(&mut world, reward.holder_accounts[1]).await,
        2_000
    );
    assert_eq!(
        reward_balance(&mut world, vault).await,
        0,
        "paid out exactly what was funded"
    );

    // Nothing more accrues once the stream has ended.
    world.set_unix_time(T0 + 500).await;
    assert_custom_error(
        claim_rewards(&mut world, &reward, A).await,
        HolderRewardsError::NothingToClaim.code(),
    );
}

#[tokio::test]
async fn buys_and_sells_keep_the_eligible_supply_in_step() {
    let (mut world, _reward) = running().await;
    // A buy: the (never registered) pool vault pays B.
    transfer(&mut world, POOL, B, 500).await.expect("buy");
    assert_eq!(global(&mut world).await.stream.eligible_supply, 1_500);
    assert_eq!(record(&mut world, B).await.holder.checkpoint, 500);
    // A sell: A pays the pool back.
    transfer(&mut world, A, POOL, 300).await.expect("sell");
    assert_eq!(global(&mut world).await.stream.eligible_supply, 1_200);
    // A move to an unregistered wallet takes the amount out of the counted supply.
    transfer(&mut world, A, C, 200)
        .await
        .expect("A to unregistered C");
    assert_eq!(global(&mut world).await.stream.eligible_supply, 1_000);
}

#[tokio::test]
async fn an_unregistered_account_earns_nothing() {
    let (mut world, reward) = running().await;
    transfer(&mut world, A, C, 1_000)
        .await
        .expect("all of A's tokens to C");
    world.set_unix_time(T0 + 100).await;
    assert_custom_error(
        claim_rewards(&mut world, &reward, C).await,
        HolderRewardsError::NotRegistered.code(),
    );
    // A no longer holds anything, so it earned only until the transfer, which was at the same
    // instant as the funding.
    assert_eq!(record(&mut world, A).await.holder.checkpoint, 0);
}

#[tokio::test]
async fn claims_are_guarded() {
    let (mut world, reward) = running().await;
    world.set_unix_time(T0 + 100).await;

    // B signing for A's token account.
    let ix = claim(
        &program_id(),
        &world.owner(B),
        &world.mint.pubkey(),
        &world.account(A),
        &reward.holder_accounts[B - 1],
        &reward.mint.pubkey(),
        &reward.token_program,
    );
    let b = clone(&world.owners[B]);
    assert_custom_error(
        world.send(&[ix], &[&b]).await,
        HolderRewardsError::WrongOwner.code(),
    );

    // A's earnings paid into B's reward account.
    let ix = claim(
        &program_id(),
        &world.owner(A),
        &world.mint.pubkey(),
        &world.account(A),
        &reward.holder_accounts[B - 1],
        &reward.mint.pubkey(),
        &reward.token_program,
    );
    let a = clone(&world.owners[A]);
    assert_custom_error(
        world.send(&[ix], &[&a]).await,
        HolderRewardsError::RewardAccountMismatch.code(),
    );

    // The proper claim works, and a second one right after has nothing to pay.
    claim_rewards(&mut world, &reward, A).await.expect("claim");
    assert_custom_error(
        claim_rewards(&mut world, &reward, A).await,
        HolderRewardsError::NothingToClaim.code(),
    );
}

#[tokio::test]
async fn funding_validates_amount_and_duration_and_rolls_over() {
    let (mut world, reward) = running().await;
    assert_custom_error(
        fund_rewards(&mut world, &reward, 0, 10).await,
        HolderRewardsError::ZeroAmount.code(),
    );
    assert_custom_error(
        fund_rewards(&mut world, &reward, 10, 0).await,
        HolderRewardsError::InvalidDuration.code(),
    );
    // Half way through, add 5,000 over 100 more seconds: 5,000 unpaid + 5,000 new = 100/s again.
    world.set_unix_time(T0 + 50).await;
    fund_rewards(&mut world, &reward, 5_000, 100)
        .await
        .expect("top up");
    let stream = global(&mut world).await.stream;
    assert_eq!((stream.rate, stream.period_finish), (100, T0 + 150));
}

#[tokio::test]
async fn a_classic_spl_token_works_as_the_reward() {
    let mut world = world_with_holders(true).await;
    let reward = create_reward(&mut world, spl_token::id()).await;
    init(&mut world, &reward).await.expect("initialize");
    register_holder(&mut world, A).await.expect("register A");
    fund_rewards(&mut world, &reward, 1_000, 10)
        .await
        .expect("fund");
    world.set_unix_time(T0 + 10).await;
    claim_rewards(&mut world, &reward, A).await.expect("claim");
    assert_eq!(
        reward_balance(&mut world, reward.holder_accounts[0]).await,
        1_000
    );
}

#[tokio::test]
async fn a_direct_execute_call_is_refused() {
    let (mut world, _reward) = running().await;
    let mint = world.mint.pubkey();
    let mut ix = spl_transfer_hook_interface::instruction::execute(
        &program_id(),
        &world.account(A),
        &mint,
        &world.account(B),
        &world.owner(A),
        1,
    );
    ix.accounts.push(AccountMeta::new_readonly(
        validation_list_address(&mint, &program_id()).0,
        false,
    ));
    for key in writable(&world, A, B) {
        ix.accounts.push(AccountMeta::new(key, false));
    }
    assert_custom_error(
        world.send(&[ix], &[]).await,
        KitError::NotDirectInvocation.code(),
    );
}

#[tokio::test]
async fn only_the_mints_hook_authority_can_initialize() {
    let mut world = world_with_holders(true).await;
    let reward = create_reward(&mut world, spl_token_2022::id()).await;
    let impostor = Keypair::new();
    let ix = initialize(
        &program_id(),
        &world.payer(),
        &impostor.pubkey(),
        &world.mint.pubkey(),
        &world.account(POOL),
        &reward.mint.pubkey(),
        &reward.token_program,
    );
    assert_custom_error(
        world.send(&[ix], &[&impostor]).await,
        KitError::AuthorityMismatch.code(),
    );
    // Nothing was created: the real authority can still initialize.
    init(&mut world, &reward).await.expect("initialize");
}
