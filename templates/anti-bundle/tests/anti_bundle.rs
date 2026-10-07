//! The hook inside real Token-2022 transfers: the per-slot budget, which transfers count as buys,
//! setup validation and direct-call refusal.
//!
//! Accounts: 0 and 1 are venue vaults A and B, 2 is a buyer, 3 another wallet. Runs the hook
//! natively by default; set `SBF_OUT_DIR` to a directory holding `anti_bundle_hook.so` to run the
//! real SBF build.

use anti_bundle_hook::{
    error::AntiBundleError,
    instruction::initialize,
    process_instruction,
    rule::Params,
    state::{config_address, counter_address, Config, Counter},
};
use hook_kit::{
    testing::{assert_custom_error, AccountSpec, World},
    validation_list_address, KitError,
};
use solana_program_test::{processor, BanksClientError, ProgramTest};
use solana_sdk::{
    instruction::AccountMeta,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

const VENUE_A: usize = 0;
const VENUE_B: usize = 1;
const BUYER: usize = 2;
const OTHER: usize = 3;

const END: i64 = 2_000;
const NOW: i64 = 1_500;

const PARAMS: Params = Params {
    active_until: END,
    max_buys_per_slot: 2,
};

fn program_id() -> Pubkey {
    Pubkey::new_from_array([0xAB; 32])
}

fn counter(world: &World) -> Pubkey {
    counter_address(&world.mint.pubkey(), &program_id()).0
}

async fn fresh() -> World {
    let id = program_id();
    let test = ProgramTest::new("anti_bundle_hook", id, processor!(process_instruction));
    World::start(
        test,
        id,
        vec![
            AccountSpec::owned_by(Keypair::new(), 10_000_000),
            AccountSpec::owned_by(Keypair::new(), 10_000_000),
            AccountSpec::owned_by(Keypair::new(), 0),
            AccountSpec::owned_by(Keypair::new(), 0),
        ],
    )
    .await
}

async fn launched(params: Params) -> World {
    let mut world = fresh().await;
    let ix = initialize(
        &program_id(),
        &world.payer(),
        &world.payer(),
        &world.mint.pubkey(),
        &[world.account(VENUE_A), world.account(VENUE_B)],
        params,
    );
    world.send(&[ix], &[]).await.expect("initialize");
    world.set_unix_time(NOW).await;
    world
}

/// Move to a fresh slot (the budget starts over), keeping the clock.
async fn new_slot(world: &mut World) {
    let clock: solana_sdk::clock::Clock = world.context.banks_client.get_sysvar().await.unwrap();
    world.context.warp_to_slot(clock.slot + 2).unwrap();
    world.set_unix_time(NOW).await;
}

/// One transaction holding `count` buys of 10 from `venue` to the buyer.
async fn buys(world: &mut World, venue: usize, count: usize) -> Result<(), BanksClientError> {
    let counter = counter(world);
    let mut instructions = Vec::new();
    for _ in 0..count {
        instructions.push(world.transfer_ix(venue, BUYER, 10, &[counter]).await);
    }
    let owner = Keypair::from_bytes(&world.owners[venue].to_bytes()).unwrap();
    world.send(&instructions, &[&owner]).await
}

#[tokio::test]
async fn initialize_records_the_budget_and_validates_its_inputs() {
    let mut world = fresh().await;
    let mint = world.mint.pubkey();
    let id = program_id();
    let venues = [world.account(VENUE_A), world.account(VENUE_B)];
    let init = |world: &World, authority: &Pubkey, venues: &[Pubkey], params| {
        initialize(&id, &world.payer(), authority, &mint, venues, params)
    };

    let ix = init(
        &world,
        &world.payer(),
        &venues,
        Params {
            max_buys_per_slot: 0,
            ..PARAMS
        },
    );
    assert_custom_error(
        world.send(&[ix], &[]).await,
        AntiBundleError::InvalidParams.code(),
    );
    let ix = init(&world, &world.payer(), &[], PARAMS);
    assert_custom_error(
        world.send(&[ix], &[]).await,
        AntiBundleError::InvalidVenues.code(),
    );
    let ix = init(&world, &world.payer(), &[venues[0], venues[0]], PARAMS);
    assert_custom_error(
        world.send(&[ix], &[]).await,
        AntiBundleError::InvalidVenues.code(),
    );
    // The venue must be a token account of this mint: an arbitrary wallet is not.
    let ix = init(&world, &world.payer(), &[world.payer()], PARAMS);
    assert!(world.send(&[ix], &[]).await.is_err());
    let impostor = Keypair::new();
    let ix = init(&world, &impostor.pubkey(), &venues, PARAMS);
    assert_custom_error(
        world.send(&[ix], &[&impostor]).await,
        KitError::AuthorityMismatch.code(),
    );

    let ix = init(&world, &world.payer(), &venues, PARAMS);
    world.send(&[ix], &[]).await.expect("initialize");
    let stored = Config::decode(&world.data(config_address(&mint, &id).0).await).unwrap();
    assert_eq!(stored.params, PARAMS);
    assert_eq!(stored.venues, venues.to_vec());
    let stored_counter = Counter::decode(&world.data(counter_address(&mint, &id).0).await).unwrap();
    assert_eq!(stored_counter.buys, 0);

    let again = init(&world, &world.payer(), &venues, PARAMS);
    assert_custom_error(
        world.send(&[again], &[]).await,
        KitError::AlreadyInitialized.code(),
    );
}

#[tokio::test]
async fn buys_fit_the_budget_and_one_more_in_the_slot_is_refused_as_a_whole() {
    let mut world = launched(PARAMS).await;
    buys(&mut world, VENUE_A, 2).await.expect("two buys fit");
    assert_eq!(world.balance(BUYER).await, 20);

    new_slot(&mut world).await;
    let before = world.balance(BUYER).await;
    assert_custom_error(
        buys(&mut world, VENUE_A, 3).await,
        AntiBundleError::TooManyBuysInSlot.code(),
    );
    assert_eq!(
        world.balance(BUYER).await,
        before,
        "the bundle must roll back completely"
    );
    let stored = Counter::decode(&world.data(counter(&world)).await).unwrap();
    assert!(
        stored.buys <= 2,
        "a refused bundle must not leave counts behind"
    );
}

#[tokio::test]
async fn the_budget_starts_over_in_the_next_slot() {
    let mut world = launched(PARAMS).await;
    buys(&mut world, VENUE_A, 2).await.expect("a full budget");
    new_slot(&mut world).await;
    buys(&mut world, VENUE_A, 2)
        .await
        .expect("a fresh slot, a fresh budget");
    assert_eq!(world.balance(BUYER).await, 40);
}

#[tokio::test]
async fn every_recognised_venue_draws_on_the_same_budget() {
    let mut world = launched(PARAMS).await;
    let counter = counter(&world);
    let a = world.transfer_ix(VENUE_A, BUYER, 10, &[counter]).await;
    let b1 = world.transfer_ix(VENUE_B, BUYER, 10, &[counter]).await;
    let b2 = world.transfer_ix(VENUE_B, BUYER, 10, &[counter]).await;
    let owner_a = Keypair::from_bytes(&world.owners[VENUE_A].to_bytes()).unwrap();
    let owner_b = Keypair::from_bytes(&world.owners[VENUE_B].to_bytes()).unwrap();
    // One buy from A and two from B are three buys in one slot.
    assert_custom_error(
        world.send(&[a, b1, b2], &[&owner_a, &owner_b]).await,
        AntiBundleError::TooManyBuysInSlot.code(),
    );
}

#[tokio::test]
async fn selling_and_moving_tokens_are_never_counted() {
    let mut world = launched(PARAMS).await;
    buys(&mut world, VENUE_A, 2)
        .await
        .expect("the slot's budget");
    let counter = counter(&world);
    // The budget is spent, yet in the same slot a sell back to the pool, and a wallet-to-wallet
    // move, still go through: only transfers out of a venue are buys.
    world
        .transfer(BUYER, VENUE_A, 10, &[counter])
        .await
        .expect("sell");
    world
        .transfer(BUYER, OTHER, 10, &[counter])
        .await
        .expect("move");
    assert_eq!(world.balance(OTHER).await, 10);
}

#[tokio::test]
async fn nothing_is_counted_after_the_rule_ends_and_zero_means_never() {
    let mut world = launched(PARAMS).await;
    world.set_unix_time(END).await;
    buys(&mut world, VENUE_A, 5).await.expect("after the end");

    let mut forever = launched(Params {
        active_until: 0,
        ..PARAMS
    })
    .await;
    forever.set_unix_time(i64::MAX / 2).await;
    assert_custom_error(
        buys(&mut forever, VENUE_A, 3).await,
        AntiBundleError::TooManyBuysInSlot.code(),
    );
}

#[tokio::test]
async fn a_direct_execute_call_is_refused() {
    let mut world = launched(PARAMS).await;
    let mint = world.mint.pubkey();
    let mut ix = spl_transfer_hook_interface::instruction::execute(
        &program_id(),
        &world.account(VENUE_A),
        &mint,
        &world.account(BUYER),
        &world.owner(VENUE_A),
        1,
    );
    ix.accounts.push(AccountMeta::new_readonly(
        validation_list_address(&mint, &program_id()).0,
        false,
    ));
    ix.accounts.push(AccountMeta::new_readonly(
        config_address(&mint, &program_id()).0,
        false,
    ));
    ix.accounts.push(AccountMeta::new(counter(&world), false));
    assert_custom_error(
        world.send(&[ix], &[]).await,
        KitError::NotDirectInvocation.code(),
    );
}
