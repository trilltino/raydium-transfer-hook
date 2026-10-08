//! The one-time mode (a spin-off) inside real Token-2022 transfers: a parent token whose holders earn a one-time child
//! allocation, with exact payouts, the single-funding rule, and the guards inherited from the
//! balance-time accounting.
//!
//! Account indices: 0 is the pool vault, 1..=3 are holders A, B, C. Runs the hook natively by
//! default; set `SBF_OUT_DIR` to a directory holding `holder_rewards_hook.so` to run the real SBF
//! build.

use holder_rewards_hook::{
    error::HolderRewardsError,
    instruction::{claim, fund, initialize_one_time, register},
    process_instruction,
    state::{global_address, record_address, reward_vault_address},
};
use hook_kit::{
    testing::{assert_custom_error, AccountSpec, PlainToken, World},
    validation_list_address, KitError,
};
use solana_program_test::{processor, BanksClientError, ProgramTest};
use solana_sdk::{
    instruction::AccountMeta,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

const POOL: usize = 0;
const A: usize = 1;
const B: usize = 2;
const C: usize = 3;

const T0: i64 = 1_000;

/// The loyalty accounting's error codes this template inherits.
const LOYALTY_EXCLUDED_ACCOUNT: u32 = 0xC004;
const LOYALTY_REWARD_MINT_HAS_HOOK: u32 = 0xC009;

fn program_id() -> Pubkey {
    Pubkey::new_from_array([0xE5; 32])
}

fn clone(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).unwrap()
}

/// A parent token (hooked), and a plain child token to spin off.
async fn world_and_child(token_program: Pubkey) -> (World, PlainToken) {
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
    world.revoke_mint_authority().await;
    world.set_unix_time(T0).await;
    let child = world
        .create_plain_token(token_program, &[A, B, C], 1_000_000)
        .await;
    (world, child)
}

async fn init(world: &mut World, child: &PlainToken) -> Result<(), BanksClientError> {
    let ix = initialize_one_time(
        &program_id(),
        &world.payer(),
        &world.payer(),
        &world.mint.pubkey(),
        &world.account(POOL),
        &child.mint.pubkey(),
        &child.token_program,
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

async fn fund_allocation(
    world: &mut World,
    child: &PlainToken,
    amount: u64,
    duration: u32,
) -> Result<(), BanksClientError> {
    let ix = fund(
        &program_id(),
        &world.payer(),
        &child.funder_account,
        &world.mint.pubkey(),
        &child.mint.pubkey(),
        &child.token_program,
        amount,
        duration,
    );
    world.send(&[ix], &[]).await
}

async fn claim_child(
    world: &mut World,
    child: &PlainToken,
    holder: usize,
) -> Result<(), BanksClientError> {
    let ix = claim(
        &program_id(),
        &world.owner(holder),
        &world.mint.pubkey(),
        &world.account(holder),
        &child.holder_accounts[holder - 1],
        &child.mint.pubkey(),
        &child.token_program,
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

async fn move_parent(
    world: &mut World,
    from: usize,
    to: usize,
    amount: u64,
) -> Result<(), BanksClientError> {
    let extras = writable(world, from, to);
    world.transfer(from, to, amount, &extras).await
}

/// A spin-off in progress: A and B registered, 10,000 child tokens funded over 100 seconds.
async fn running() -> (World, PlainToken) {
    let (mut world, child) = world_and_child(spl_token_2022::id()).await;
    init(&mut world, &child).await.expect("initialize");
    register_holder(&mut world, A).await.expect("register A");
    register_holder(&mut world, B).await.expect("register B");
    fund_allocation(&mut world, &child, 10_000, 100)
        .await
        .expect("fund");
    (world, child)
}

#[tokio::test]
async fn history_stays_with_the_seller_and_the_future_follows_the_buyer() {
    let (mut world, child) = running().await;
    let vault = reward_vault_address(&world.mint.pubkey(), &program_id()).0;
    assert_eq!(world.token_balance(vault).await, 10_000);

    // A holds all 1,000 parent tokens for half the window, then sells everything to B.
    world.set_unix_time(T0 + 50).await;
    move_parent(&mut world, A, B, 1_000)
        .await
        .expect("A sells to B");

    world.set_unix_time(T0 + 100).await;
    claim_child(&mut world, &child, A).await.expect("claim A");
    claim_child(&mut world, &child, B).await.expect("claim B");
    assert_eq!(world.token_balance(child.holder_accounts[0]).await, 5_000);
    assert_eq!(world.token_balance(child.holder_accounts[1]).await, 5_000);
    assert_eq!(
        world.token_balance(vault).await,
        0,
        "the allocation is paid out exactly"
    );
}

#[tokio::test]
async fn the_allocation_can_only_be_funded_once() {
    let (mut world, child) = running().await;
    assert_custom_error(
        fund_allocation(&mut world, &child, 5_000, 100).await,
        HolderRewardsError::AlreadyFunded.code(),
    );
    // Not even after the window has ended.
    world.set_unix_time(T0 + 1_000).await;
    assert_custom_error(
        fund_allocation(&mut world, &child, 5_000, 100).await,
        HolderRewardsError::AlreadyFunded.code(),
    );
}

#[tokio::test]
async fn nothing_is_paid_to_a_holder_who_never_registered_or_to_the_pool() {
    let (mut world, child) = running().await;
    // C receives parent tokens but never registers: it has no record, so it cannot claim.
    move_parent(&mut world, A, C, 1_000)
        .await
        .expect("A to unregistered C");
    world.set_unix_time(T0 + 100).await;
    assert_custom_error(
        claim_child(&mut world, &child, C).await,
        0xC005, // holder-rewards' NotRegistered
    );
    // And the pool's vault can never register.
    assert_custom_error(
        register_holder(&mut world, POOL).await,
        LOYALTY_EXCLUDED_ACCOUNT,
    );
}

#[tokio::test]
async fn a_child_token_with_a_hook_of_its_own_is_refused() {
    let (mut world, child) = world_and_child(spl_token_2022::id()).await;
    // The hooked parent mint stands in for a child that carries a TransferHook.
    let hooked_child = PlainToken {
        mint: clone(&world.mint),
        token_program: spl_token_2022::id(),
        funder_account: child.funder_account,
        holder_accounts: child.holder_accounts.clone(),
    };
    assert_custom_error(
        init(&mut world, &hooked_child).await,
        LOYALTY_REWARD_MINT_HAS_HOOK,
    );
}

#[tokio::test]
async fn a_classic_spl_token_works_as_the_child() {
    let (mut world, child) = world_and_child(spl_token::id()).await;
    init(&mut world, &child).await.expect("initialize");
    register_holder(&mut world, A).await.expect("register A");
    fund_allocation(&mut world, &child, 1_000, 10)
        .await
        .expect("fund");
    world.set_unix_time(T0 + 10).await;
    claim_child(&mut world, &child, A).await.expect("claim");
    assert_eq!(world.token_balance(child.holder_accounts[0]).await, 1_000);
}

#[tokio::test]
async fn a_direct_execute_call_is_refused() {
    let (mut world, _child) = running().await;
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
