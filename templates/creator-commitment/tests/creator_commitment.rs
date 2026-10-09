//! The hook inside a real Token-2022 transfer: the rule, setup validation, and direct-call refusal.
//!
//! Runs the hook natively by default; set `SBF_OUT_DIR` to a directory holding
//! `creator_commitment_hook.so` to run the real SBF build instead.

use creator_commitment_hook::{
    config::{config_address, Config},
    error::CommitmentError,
    instruction::initialize,
    process_instruction,
    rule::Schedule,
};
use hook_kit::{
    testing::{assert_custom_error, AccountSpec, World},
    validation_list_address, KitError,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    instruction::AccountMeta,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

const CREATOR: usize = 0;
const HOLDER: usize = 1;
const OTHER: usize = 2;

/// 600 of the creator's 1000 tokens are locked.
const SCHEDULE: Schedule = Schedule {
    locked_total: 600,
    start: 1_000,
    cliff: 2_000,
    end: 11_000,
};

fn program_id() -> Pubkey {
    Pubkey::new_from_array([0xC0; 32])
}

async fn world() -> World {
    let id = program_id();
    let test = ProgramTest::new(
        "creator_commitment_hook",
        id,
        processor!(process_instruction),
    );
    World::start(
        test,
        id,
        vec![
            AccountSpec::owned_by(Keypair::new(), 1_000),
            AccountSpec::owned_by(Keypair::new(), 100),
            AccountSpec::owned_by(Keypair::new(), 0),
        ],
    )
    .await
}

async fn committed(schedule: Schedule) -> World {
    let mut world = world().await;
    let ix = initialize(
        &program_id(),
        &world.payer(),
        &world.payer(),
        &world.mint.pubkey(),
        &world.account(CREATOR),
        schedule,
    );
    world.send(&[ix], &[]).await.expect("initialize");
    world
}

#[tokio::test]
async fn initialize_records_the_schedule_and_the_creator_account() {
    let mut world = committed(SCHEDULE).await;
    let (config, _) = config_address(&world.mint.pubkey(), &program_id());
    let stored = Config::decode(&world.data(config).await).unwrap();
    assert_eq!(stored.schedule, SCHEDULE);
    assert_eq!(stored.creator_account, world.account(CREATOR));
    assert_eq!(stored.mint, world.mint.pubkey());
}

#[tokio::test]
async fn initialize_rejects_bad_schedules_and_balances_with_exact_codes() {
    let mut world = world().await;
    let init = |world: &World, schedule| {
        initialize(
            &program_id(),
            &world.payer(),
            &world.payer(),
            &world.mint.pubkey(),
            &world.account(CREATOR),
            schedule,
        )
    };

    let ix = init(
        &world,
        Schedule {
            locked_total: 0,
            ..SCHEDULE
        },
    );
    assert_custom_error(
        world.send(&[ix], &[]).await,
        CommitmentError::ZeroLockedAmount.code(),
    );
    let ix = init(
        &world,
        Schedule {
            end: 500,
            ..SCHEDULE
        },
    );
    assert_custom_error(
        world.send(&[ix], &[]).await,
        CommitmentError::InvalidSchedule.code(),
    );
    let ix = init(
        &world,
        Schedule {
            locked_total: 1_001,
            ..SCHEDULE
        },
    );
    assert_custom_error(
        world.send(&[ix], &[]).await,
        CommitmentError::InsufficientBalanceAtInit.code(),
    );

    // Not the mint's hook authority.
    let impostor = Keypair::new();
    let ix = initialize(
        &program_id(),
        &world.payer(),
        &impostor.pubkey(),
        &world.mint.pubkey(),
        &world.account(CREATOR),
        SCHEDULE,
    );
    assert_custom_error(
        world.send(&[ix], &[&impostor]).await,
        KitError::AuthorityMismatch.code(),
    );

    // None of the failures left anything behind, so a correct call still works, once.
    let ix = init(&world, SCHEDULE);
    world.send(&[ix], &[]).await.expect("initialize");
    let again = init(&world, SCHEDULE);
    assert_custom_error(
        world.send(&[again], &[]).await,
        KitError::AlreadyInitialized.code(),
    );
}

#[tokio::test]
async fn before_the_cliff_the_creator_can_only_move_what_is_above_the_floor() {
    let mut world = committed(SCHEDULE).await;
    world.set_unix_time(1_500).await;

    // 1000 held, 600 locked: 400 may leave, landing exactly on the floor.
    world
        .transfer(CREATOR, HOLDER, 400, &[])
        .await
        .expect("down to the floor");
    assert_eq!(world.balance(CREATOR).await, 600);

    // One more token would breach it, and nothing moves.
    let before = (world.balance(CREATOR).await, world.balance(HOLDER).await);
    assert_custom_error(
        world.transfer(CREATOR, HOLDER, 1, &[]).await,
        CommitmentError::VestingFloorBreached.code(),
    );
    assert_eq!(
        (world.balance(CREATOR).await, world.balance(HOLDER).await),
        before,
        "a refused transfer must roll back completely"
    );
}

#[tokio::test]
async fn tokens_unlock_linearly_after_the_cliff() {
    let mut world = committed(SCHEDULE).await;
    world
        .transfer(CREATOR, HOLDER, 400, &[])
        .await
        .expect("above the floor");

    // Halfway through [1000, 11000]: half of 600 has unlocked, so 300 are still locked.
    world.set_unix_time(6_000).await;
    world
        .transfer(CREATOR, HOLDER, 300, &[])
        .await
        .expect("the unlocked half");
    assert_eq!(world.balance(CREATOR).await, 300);
    assert_custom_error(
        world.transfer(CREATOR, HOLDER, 1, &[]).await,
        CommitmentError::VestingFloorBreached.code(),
    );
}

#[tokio::test]
async fn after_the_end_everything_can_leave() {
    let mut world = committed(SCHEDULE).await;
    world.set_unix_time(11_000).await;
    world
        .transfer(CREATOR, HOLDER, 1_000, &[])
        .await
        .expect("fully unlocked");
    assert_eq!(world.balance(CREATOR).await, 0);
}

#[tokio::test]
async fn other_holders_are_unaffected_and_the_creator_can_receive() {
    let mut world = committed(SCHEDULE).await;
    world.set_unix_time(1_500).await;

    // A holder sends all they have, freely, to anyone, including the creator's account.
    world
        .transfer(HOLDER, OTHER, 60, &[])
        .await
        .expect("holder to other");
    world
        .transfer(HOLDER, CREATOR, 40, &[])
        .await
        .expect("creator receives");
    assert_eq!(world.balance(CREATOR).await, 1_040);
    // The extra 40 is above the floor and can leave; the floor itself still cannot.
    world
        .transfer(CREATOR, OTHER, 440, &[])
        .await
        .expect("down to the floor");
    assert_custom_error(
        world.transfer(CREATOR, OTHER, 1, &[]).await,
        CommitmentError::VestingFloorBreached.code(),
    );
}

#[tokio::test]
async fn a_direct_execute_call_is_refused() {
    let mut world = committed(SCHEDULE).await;
    let mint = world.mint.pubkey();
    let mut ix = spl_transfer_hook_interface::instruction::execute(
        &program_id(),
        &world.account(CREATOR),
        &mint,
        &world.account(HOLDER),
        &world.owner(CREATOR),
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
    // Only Token-2022 sets the `transferring` flag, so a direct call never gets to the rule.
    assert_custom_error(
        world.send(&[ix], &[]).await,
        KitError::NotDirectInvocation.code(),
    );
}

// ---------------------------------------------------------------------------------------------
// Ways around the floor that the rule's documentation talks about. Each is run for real so the
// documentation states what happens, not what we hope happens.
// ---------------------------------------------------------------------------------------------

use solana_sdk::instruction::Instruction;
use spl_token_2022::instruction::{self as token_instruction, AuthorityType};

fn copy_of(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("keypair bytes")
}

/// A transfer out of `from` signed by `authority` (an owner or a delegate) instead of the account's
/// owner. The hook's accounts are resolved as for the owner; the hook does not depend on who signs.
async fn transfer_signed_by(
    world: &mut World,
    from: usize,
    to: usize,
    amount: u64,
    authority: &Keypair,
) -> Result<(), solana_program_test::BanksClientError> {
    let mut ix: Instruction = world.transfer_ix(from, to, amount, &[]).await;
    // transfer_checked: source, mint, destination, authority (signer), then the hook's accounts.
    ix.accounts[3] = AccountMeta::new_readonly(authority.pubkey(), true);
    world.send(&[ix], &[authority]).await
}

#[tokio::test]
async fn handing_the_account_to_a_new_owner_keeps_the_floor() {
    let mut world = committed(SCHEDULE).await;
    world.set_unix_time(1_500).await;

    // The creator hands the whole account, locked tokens included, to a new wallet.
    let old_owner = copy_of(&world.owners[CREATOR]);
    let new_owner = Keypair::new();
    let hand_over = token_instruction::set_authority(
        &spl_token_2022::id(),
        &world.account(CREATOR),
        Some(&new_owner.pubkey()),
        AuthorityType::AccountOwner,
        &old_owner.pubkey(),
        &[],
    )
    .unwrap();
    world
        .send(&[hand_over], &[&old_owner])
        .await
        .expect("the owner of a plain token account can be changed");

    // The floor belongs to the account, so the new owner is bound by it exactly as the old one was:
    // 400 above the floor may leave, one more may not.
    assert_custom_error(
        transfer_signed_by(&mut world, CREATOR, HOLDER, 401, &new_owner).await,
        CommitmentError::VestingFloorBreached.code(),
    );
    transfer_signed_by(&mut world, CREATOR, HOLDER, 400, &new_owner)
        .await
        .expect("the part above the floor is free");
    assert_eq!(world.balance(CREATOR).await, 600);
    // And the previous owner has no say any more.
    assert!(
        transfer_signed_by(&mut world, CREATOR, HOLDER, 1, &old_owner)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_delegate_cannot_move_the_locked_tokens_either() {
    let mut world = committed(SCHEDULE).await;
    world.set_unix_time(1_500).await;

    // The creator lets another key spend the whole account.
    let owner = copy_of(&world.owners[CREATOR]);
    let delegate = Keypair::new();
    let approve = token_instruction::approve(
        &spl_token_2022::id(),
        &world.account(CREATOR),
        &delegate.pubkey(),
        &owner.pubkey(),
        &[],
        1_000,
    )
    .unwrap();
    world
        .send(&[approve], &[&owner])
        .await
        .expect("approve a delegate");

    // A delegate's transfer still goes through the hook, so the floor holds for it too.
    assert_custom_error(
        transfer_signed_by(&mut world, CREATOR, HOLDER, 401, &delegate).await,
        CommitmentError::VestingFloorBreached.code(),
    );
    transfer_signed_by(&mut world, CREATOR, HOLDER, 400, &delegate)
        .await
        .expect("a delegate may move what is above the floor");
    assert_eq!(world.balance(CREATOR).await, 600);
}

#[tokio::test]
async fn burning_locked_tokens_is_not_stopped_and_strands_the_creator_until_the_end() {
    let mut world = committed(SCHEDULE).await;
    world.set_unix_time(1_500).await;

    // Burn is not a transfer, so Token-2022 never calls the hook for it: the creator can destroy
    // locked tokens. (This is the documented gap; the point of this test is to show its extent.)
    let owner = copy_of(&world.owners[CREATOR]);
    let burn = token_instruction::burn(
        &spl_token_2022::id(),
        &world.account(CREATOR),
        &world.mint.pubkey(),
        &owner.pubkey(),
        &[],
        600,
    )
    .unwrap();
    world
        .send(&[burn], &[&owner])
        .await
        .expect("burn is not blocked");
    assert_eq!(world.balance(CREATOR).await, 400);

    // But it only hurts the creator: the balance is now below the floor, so nothing can leave
    // until the schedule has unlocked...
    assert_custom_error(
        world.transfer(CREATOR, HOLDER, 1, &[]).await,
        CommitmentError::VestingFloorBreached.code(),
    );
    // ...and burning does not release anything to anyone else. After the end the rest is free.
    world.set_unix_time(11_000).await;
    world
        .transfer(CREATOR, HOLDER, 400, &[])
        .await
        .expect("fully unlocked");
}

#[tokio::test]
async fn a_schedule_spanning_the_whole_i64_range_still_enforces_the_floor() {
    // `now - start` and `end - start` overflow an i64 here. The program is built with overflow
    // checks, so evaluating this with plain subtraction would abort every transfer; instead the
    // floor is exact: at the unix time the test bank starts at, just under half is unlocked.
    let mut world = committed(Schedule {
        locked_total: 600,
        start: i64::MIN,
        cliff: i64::MIN,
        end: i64::MAX,
    })
    .await;
    world.set_unix_time(0).await;
    // 1000 held, 300 locked (half of 600 at the midpoint): 700 may leave, not 701.
    world
        .transfer(CREATOR, HOLDER, 700, &[])
        .await
        .expect("down to the floor");
    assert_custom_error(
        world.transfer(CREATOR, HOLDER, 1, &[]).await,
        CommitmentError::VestingFloorBreached.code(),
    );
}
