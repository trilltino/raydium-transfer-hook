//! The hook inside real Token-2022 transfers: each check of the rule, the window, which transfers
//! are not buys, setup validation, and direct-call refusal.
//!
//! A "buy" here is a transfer out of the pool vault (account 0). Runs the hook natively by default;
//! set `SBF_OUT_DIR` to a directory holding `fair_launch_hook.so` to run the real SBF build.

use fair_launch_hook::{
    config::{config_address, counter_address, Config, Counter},
    error::FairLaunchError,
    instruction::initialize,
    process_instruction,
    rule::Params,
};
use hook_kit::{
    testing::{assert_custom_error, AccountSpec, World},
    validation_list_address, KitError,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
};

const VAULT: usize = 0;
const BUYER: usize = 1;
const OTHER: usize = 2;

const START: i64 = 1_000;
const END: i64 = 2_000;
const IN_WINDOW: i64 = 1_500;

const PARAMS: Params = Params {
    window_start: START,
    window_end: END,
    max_buy: 50,
    max_wallet: 120,
    max_buys_per_slot: 2,
    max_priority_micro_lamports: 1_000,
};

fn program_id() -> Pubkey {
    Pubkey::new_from_array([0xF1; 32])
}

fn counter(world: &World) -> Pubkey {
    counter_address(&world.mint.pubkey(), &program_id()).0
}

async fn launched(params: Params) -> World {
    let id = program_id();
    let test = ProgramTest::new("fair_launch_hook", id, processor!(process_instruction));
    let mut world = World::start(
        test,
        id,
        vec![
            AccountSpec::owned_by(Keypair::new(), 10_000_000),
            AccountSpec::owned_by(Keypair::new(), 0),
            AccountSpec::owned_by(Keypair::new(), 0),
        ],
    )
    .await;
    let ix = initialize(
        &id,
        &world.payer(),
        &world.payer(),
        &world.mint.pubkey(),
        &world.account(VAULT),
        params,
    );
    world.send(&[ix], &[]).await.expect("initialize");
    world.set_unix_time(IN_WINDOW).await;
    world
}

/// Move to a fresh slot (the per-slot budget starts over), still inside the window.
async fn new_slot(world: &mut World) {
    let clock: solana_sdk::clock::Clock = world.context.banks_client.get_sysvar().await.unwrap();
    world.context.warp_to_slot(clock.slot + 2).unwrap();
    world.set_unix_time(IN_WINDOW).await;
}

async fn buy(
    world: &mut World,
    to: usize,
    amount: u64,
) -> Result<(), solana_program_test::BanksClientError> {
    let counter = counter(world);
    world.transfer(VAULT, to, amount, &[counter]).await
}

/// One transaction holding `count` buys of `amount` (all in the same slot, necessarily), with
/// `before` instructions in front.
async fn bundle(
    world: &mut World,
    before: Vec<Instruction>,
    count: usize,
    amount: u64,
) -> Result<(), solana_program_test::BanksClientError> {
    let counter = counter(world);
    let mut instructions = before;
    for _ in 0..count {
        instructions.push(world.transfer_ix(VAULT, BUYER, amount, &[counter]).await);
    }
    let owner = Keypair::from_bytes(&world.owners[VAULT].to_bytes()).unwrap();
    world.send(&instructions, &[&owner]).await
}

#[tokio::test]
async fn initialize_records_the_launch_and_validates_its_inputs() {
    let id = program_id();
    let test = ProgramTest::new("fair_launch_hook", id, processor!(process_instruction));
    let mut world = World::start(
        test,
        id,
        vec![
            AccountSpec::owned_by(Keypair::new(), 1_000),
            AccountSpec::owned_by(Keypair::new(), 0),
        ],
    )
    .await;
    let mint = world.mint.pubkey();
    let init = |world: &World, authority: &Pubkey, params| {
        initialize(
            &id,
            &world.payer(),
            authority,
            &mint,
            &world.account(0),
            params,
        )
    };

    let ix = init(
        &world,
        &world.payer(),
        Params {
            max_buy: 0,
            ..PARAMS
        },
    );
    assert_custom_error(
        world.send(&[ix], &[]).await,
        FairLaunchError::InvalidParams.code(),
    );
    let impostor = Keypair::new();
    let ix = init(&world, &impostor.pubkey(), PARAMS);
    assert_custom_error(
        world.send(&[ix], &[&impostor]).await,
        KitError::AuthorityMismatch.code(),
    );

    let ix = init(&world, &world.payer(), PARAMS);
    world.send(&[ix], &[]).await.expect("initialize");
    let (config, _) = config_address(&mint, &id);
    let stored = Config::decode(&world.data(config).await).unwrap();
    assert_eq!(stored.params, PARAMS);
    assert_eq!(stored.pool_vault, world.account(0));
    let stored_counter = Counter::decode(&world.data(counter_address(&mint, &id).0).await).unwrap();
    assert_eq!(stored_counter.buys, 0);

    let again = init(&world, &world.payer(), PARAMS);
    assert_custom_error(
        world.send(&[again], &[]).await,
        KitError::AlreadyInitialized.code(),
    );
}

#[tokio::test]
async fn a_buy_at_the_cap_passes_and_one_token_over_is_refused() {
    let mut world = launched(PARAMS).await;
    buy(&mut world, BUYER, 50).await.expect("buy at the cap");
    assert_eq!(world.balance(BUYER).await, 50);

    new_slot(&mut world).await;
    let before = (world.balance(VAULT).await, world.balance(BUYER).await);
    assert_custom_error(
        buy(&mut world, BUYER, 51).await,
        FairLaunchError::PerBuyCapExceeded.code(),
    );
    assert_eq!(
        (world.balance(VAULT).await, world.balance(BUYER).await),
        before
    );
}

#[tokio::test]
async fn a_wallet_cannot_accumulate_past_the_wallet_cap_across_buys() {
    let mut world = launched(PARAMS).await;
    buy(&mut world, BUYER, 50).await.expect("first");
    new_slot(&mut world).await;
    buy(&mut world, BUYER, 50).await.expect("second, at 100");
    new_slot(&mut world).await;
    // 100 + 21 = 121 > 120; exactly 120 is fine.
    assert_custom_error(
        buy(&mut world, BUYER, 21).await,
        FairLaunchError::MaxWalletExceeded.code(),
    );
    buy(&mut world, BUYER, 20).await.expect("up to the cap");
    assert_eq!(world.balance(BUYER).await, 120);
    // The cap is per token account: a second account starts from zero.
    new_slot(&mut world).await;
    buy(&mut world, OTHER, 50).await.expect("another account");
}

#[tokio::test]
async fn a_bundle_of_buys_in_one_slot_is_refused_as_a_whole() {
    let mut world = launched(PARAMS).await;
    bundle(&mut world, vec![], 2, 10)
        .await
        .expect("two buys fit the slot budget");
    assert_eq!(world.balance(BUYER).await, 20);

    new_slot(&mut world).await;
    let before = world.balance(BUYER).await;
    assert_custom_error(
        bundle(&mut world, vec![], 3, 10).await,
        FairLaunchError::TooManyBuysInSlot.code(),
    );
    assert_eq!(
        world.balance(BUYER).await,
        before,
        "the bundle must roll back completely"
    );
    let stored = Counter::decode(&world.data(counter(&world)).await).unwrap();
    assert!(
        stored.buys <= 2,
        "the refused bundle must not leave counts behind"
    );

    // The budget starts over in the next slot.
    new_slot(&mut world).await;
    bundle(&mut world, vec![], 2, 10)
        .await
        .expect("a fresh slot, a fresh budget");
}

#[tokio::test]
async fn a_high_priority_fee_is_refused_and_a_declared_low_one_passes() {
    let mut world = launched(PARAMS).await;
    let fee = |micro_lamports| {
        vec![ComputeBudgetInstruction::set_compute_unit_price(
            micro_lamports,
        )]
    };

    bundle(&mut world, fee(1_000), 1, 10)
        .await
        .expect("at the cap");
    new_slot(&mut world).await;
    assert_custom_error(
        bundle(&mut world, fee(1_001), 1, 10).await,
        FairLaunchError::PriorityFeeTooHigh.code(),
    );
    // A limit instruction is not a price, and a transaction that declares no fee has none to cap.
    new_slot(&mut world).await;
    bundle(
        &mut world,
        vec![ComputeBudgetInstruction::set_compute_unit_limit(400_000)],
        1,
        10,
    )
    .await
    .expect("a compute limit alone is not a fee");
    new_slot(&mut world).await;
    bundle(&mut world, vec![], 1, 10)
        .await
        .expect("no declared fee");
}

#[tokio::test]
async fn nothing_is_checked_outside_the_window() {
    let mut world = launched(PARAMS).await;
    // Before the window, and from its end onward, a buy far over every limit passes.
    world.set_unix_time(START - 1).await;
    buy(&mut world, BUYER, 5_000)
        .await
        .expect("before the window");
    world.set_unix_time(END).await;
    bundle(
        &mut world,
        vec![ComputeBudgetInstruction::set_compute_unit_price(9_999)],
        5,
        1_000,
    )
    .await
    .expect("after the window");
}

#[tokio::test]
async fn selling_and_moving_tokens_between_wallets_are_never_limited() {
    let mut world = launched(PARAMS).await;
    buy(&mut world, BUYER, 50).await.expect("buy");
    new_slot(&mut world).await;
    buy(&mut world, BUYER, 50).await.expect("buy");
    assert_eq!(world.balance(BUYER).await, 100);

    // Every transfer of the mint carries the (writable) counter, so the integrator names it for
    // sells and moves too, even though the hook only writes it on buys.
    let counter = counter(&world);
    // Selling back to the pool, above the per-buy cap, in one go.
    world
        .transfer(BUYER, VAULT, 100, &[counter])
        .await
        .expect("sell everything");
    // And a wallet-to-wallet move above the cap.
    new_slot(&mut world).await;
    buy(&mut world, BUYER, 50).await.expect("buy again");
    world
        .transfer(BUYER, OTHER, 50, &[counter])
        .await
        .expect("move tokens");
}

#[tokio::test]
async fn a_direct_execute_call_is_refused() {
    let mut world = launched(PARAMS).await;
    let mint = world.mint.pubkey();
    let mut ix = spl_transfer_hook_interface::instruction::execute(
        &program_id(),
        &world.account(VAULT),
        &mint,
        &world.account(BUYER),
        &world.owner(VAULT),
        1,
    );
    // The builder leaves the validation list and the extras to the caller.
    ix.accounts.push(AccountMeta::new_readonly(
        validation_list_address(&mint, &program_id()).0,
        false,
    ));
    ix.accounts.push(AccountMeta::new_readonly(
        config_address(&mint, &program_id()).0,
        false,
    ));
    ix.accounts.push(AccountMeta::new(counter(&world), false));
    ix.accounts
        .push(AccountMeta::new_readonly(sysvar::instructions::id(), false));
    assert_custom_error(
        world.send(&[ix], &[]).await,
        KitError::NotDirectInvocation.code(),
    );
}
