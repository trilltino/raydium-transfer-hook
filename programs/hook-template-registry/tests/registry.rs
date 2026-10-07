//! The registry in the real runtime: permissionless publishing, no squatting, authority-only
//! changes, and that a descriptor gates nothing.

use hook_template_registry::{
    descriptor::{descriptor_address, Descriptor},
    error::RegistryError,
    instruction::{close, publish, update},
    process_instruction,
};
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult};
use solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, TransactionError},
};

const KIT_ALREADY_INITIALIZED: u32 = 0x8006;
const TEMPLATE: [u8; 32] = [7; 32];

fn registry() -> Pubkey {
    Pubkey::new_from_array([0xF7; 32])
}

fn hook() -> Pubkey {
    Pubkey::new_from_array([0x11; 32])
}

fn noop(_: &Pubkey, _: &[AccountInfo], _: &[u8]) -> ProgramResult {
    Ok(())
}

fn custom(result: Result<(), BanksClientError>) -> Option<u32> {
    match result {
        Err(BanksClientError::TransactionError(TransactionError::InstructionError(
            _,
            InstructionError::Custom(code),
        ))) => Some(code),
        _ => None,
    }
}

async fn start() -> ProgramTestContext {
    let mut test = ProgramTest::new(
        "hook_template_registry",
        registry(),
        processor!(process_instruction),
    );
    // Any executable program will do as "the hook": the registry only checks it is a program. The
    // arbitrary hook is used by name so an SBF run (`SBF_OUT_DIR`) finds a real binary for it; a
    // native run uses the no-op.
    test.add_program("arbitrary_test_hook", hook(), processor!(noop));
    test.start_with_context().await
}

async fn fund(context: &mut ProgramTestContext, to: &Pubkey) {
    let ix = system_instruction::transfer(&context.payer.pubkey(), to, 1_000_000_000);
    send(context, &[ix], &[]).await.unwrap();
}

async fn send(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
    signers: &[&Keypair],
) -> Result<(), BanksClientError> {
    // A fresh blockhash each time, so an identical retry is not deduplicated by the bank.
    let slot = context.banks_client.get_root_slot().await.unwrap();
    context.warp_to_slot(slot + 2).unwrap();
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let mut all: Vec<&Keypair> = vec![&context.payer];
    all.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        &all,
        blockhash,
    );
    context.banks_client.process_transaction(tx).await
}

async fn descriptor_of(context: &mut ProgramTestContext, publisher: &Pubkey) -> Option<Descriptor> {
    let address = descriptor_address(&registry(), &hook(), &TEMPLATE, publisher).0;
    context
        .banks_client
        .get_account(address)
        .await
        .unwrap()
        .filter(|account| !account.data.is_empty())
        .map(|account| Descriptor::decode(&account.data).unwrap())
}

#[tokio::test]
async fn anyone_can_publish_a_descriptor_and_becomes_its_authority() {
    let mut context = start().await;
    let publisher = Keypair::new();
    fund(&mut context, &publisher.pubkey()).await;
    let ix = publish(
        &registry(),
        &publisher.pubkey(),
        &hook(),
        TEMPLATE,
        [9; 32],
        0b101,
    );
    send(&mut context, &[ix], &[&publisher])
        .await
        .expect("publish");

    let descriptor = descriptor_of(&mut context, &publisher.pubkey())
        .await
        .unwrap();
    assert_eq!(descriptor.hook_program, hook());
    assert_eq!(descriptor.template_id, TEMPLATE);
    assert_eq!(descriptor.manifest_hash, [9; 32]);
    assert_eq!(descriptor.template_authority, publisher.pubkey());
    assert_eq!(descriptor.flags, 0b101);
}

#[tokio::test]
async fn a_second_publisher_cannot_squat_the_first_ones_descriptor() {
    let mut context = start().await;
    let (alice, bob) = (Keypair::new(), Keypair::new());
    fund(&mut context, &alice.pubkey()).await;
    fund(&mut context, &bob.pubkey()).await;
    let ix = publish(&registry(), &alice.pubkey(), &hook(), TEMPLATE, [1; 32], 0);
    send(&mut context, &[ix], &[&alice])
        .await
        .expect("alice publishes");
    // Bob publishes the same hook and template: it is his own descriptor, at his own address.
    let ix = publish(&registry(), &bob.pubkey(), &hook(), TEMPLATE, [2; 32], 0);
    send(&mut context, &[ix], &[&bob])
        .await
        .expect("bob publishes");

    let alice_descriptor = descriptor_of(&mut context, &alice.pubkey()).await.unwrap();
    let bob_descriptor = descriptor_of(&mut context, &bob.pubkey()).await.unwrap();
    assert_eq!(alice_descriptor.template_authority, alice.pubkey());
    assert_eq!(
        alice_descriptor.manifest_hash, [1; 32],
        "alice's is untouched"
    );
    assert_eq!(bob_descriptor.template_authority, bob.pubkey());

    // And Bob cannot write to Alice's address by naming it.
    let mut forged = publish(&registry(), &bob.pubkey(), &hook(), TEMPLATE, [3; 32], 0);
    forged.accounts[2].pubkey =
        descriptor_address(&registry(), &hook(), &TEMPLATE, &alice.pubkey()).0;
    let result = send(&mut context, &[forged], &[&bob]).await;
    assert_eq!(
        custom(result),
        Some(RegistryError::InvalidDescriptor.code())
    );
}

#[tokio::test]
async fn publishing_twice_as_the_same_publisher_is_refused() {
    let mut context = start().await;
    let publisher = Keypair::new();
    fund(&mut context, &publisher.pubkey()).await;
    let ix = publish(
        &registry(),
        &publisher.pubkey(),
        &hook(),
        TEMPLATE,
        [1; 32],
        0,
    );
    send(&mut context, &[ix.clone()], &[&publisher])
        .await
        .expect("first");
    let result = send(&mut context, &[ix], &[&publisher]).await;
    assert_eq!(custom(result), Some(KIT_ALREADY_INITIALIZED));
}

#[tokio::test]
async fn only_a_program_can_be_described() {
    let mut context = start().await;
    let publisher = Keypair::new();
    fund(&mut context, &publisher.pubkey()).await;
    // A plain wallet is not an executable program.
    let not_a_program = Keypair::new().pubkey();
    let ix = publish(
        &registry(),
        &publisher.pubkey(),
        &not_a_program,
        TEMPLATE,
        [1; 32],
        0,
    );
    let result = send(&mut context, &[ix], &[&publisher]).await;
    assert_eq!(
        custom(result),
        Some(RegistryError::HookProgramNotExecutable.code())
    );
}

#[tokio::test]
async fn only_the_authority_can_update_or_close_and_closing_returns_the_rent() {
    let mut context = start().await;
    let (author, other) = (Keypair::new(), Keypair::new());
    fund(&mut context, &author.pubkey()).await;
    fund(&mut context, &other.pubkey()).await;
    let ix = publish(&registry(), &author.pubkey(), &hook(), TEMPLATE, [1; 32], 0);
    send(&mut context, &[ix], &[&author])
        .await
        .expect("publish");
    let address = descriptor_address(&registry(), &hook(), &TEMPLATE, &author.pubkey()).0;

    // Someone else cannot change it.
    let ix = update(&registry(), &other.pubkey(), &address, [5; 32], 1);
    let result = send(&mut context, &[ix], &[&other]).await;
    assert_eq!(
        custom(result),
        Some(RegistryError::NotTemplateAuthority.code())
    );
    let ix = close(&registry(), &other.pubkey(), &address);
    let result = send(&mut context, &[ix], &[&other]).await;
    assert_eq!(
        custom(result),
        Some(RegistryError::NotTemplateAuthority.code())
    );

    // The authority can.
    let ix = update(&registry(), &author.pubkey(), &address, [5; 32], 1);
    send(&mut context, &[ix], &[&author]).await.expect("update");
    let descriptor = descriptor_of(&mut context, &author.pubkey()).await.unwrap();
    assert_eq!((descriptor.manifest_hash, descriptor.flags), ([5; 32], 1));
    assert_eq!(descriptor.template_id, TEMPLATE, "the id never changes");

    let before = context
        .banks_client
        .get_balance(author.pubkey())
        .await
        .unwrap();
    let ix = close(&registry(), &author.pubkey(), &address);
    send(&mut context, &[ix], &[&author]).await.expect("close");
    assert!(descriptor_of(&mut context, &author.pubkey())
        .await
        .is_none());
    let after = context
        .banks_client
        .get_balance(author.pubkey())
        .await
        .unwrap();
    assert!(after > before, "the rent comes back (net of the fee)");

    // And after closing, the same publisher can publish again.
    let ix = publish(&registry(), &author.pubkey(), &hook(), TEMPLATE, [6; 32], 0);
    send(&mut context, &[ix], &[&author])
        .await
        .expect("republish");
}

// That a descriptor gates nothing is shown by everything else in this repository: every hook in
// `templates/` and `programs/` runs through Token-2022, CPMM and CLMM with no descriptor at all.
