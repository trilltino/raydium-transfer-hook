//! Behavior of the arbitrary third-party-style hook through the real Token-2022 processor.
//! Runs natively by default; set `SBF_OUT_DIR` to a directory holding `arbitrary_test_hook.so`
//! to run the SBF build instead.

use {
    arbitrary_test_hook::{
        init_instruction, policy_address, process_instruction, stats_address,
        validation_list_address, ArbError, Policy, Stats, EXECUTE_DISCRIMINATOR, EXTRA_ACCOUNTS,
        POLICY_LEN, STATS_LEN,
    },
    solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext},
    solana_sdk::{
        instruction::{AccountMeta, Instruction, InstructionError},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        transaction::{Transaction, TransactionError},
    },
    spl_token_2022::{
        extension::{
            transfer_hook::instruction as transfer_hook_instruction, ExtensionType,
            StateWithExtensions,
        },
        instruction as token_instruction,
        state::{Account as TokenAccount, Mint},
    },
    std::collections::HashMap,
    transfer_hook_sdk::{
        default_allowed_loaders, resolve_leg, FetchError, LegRole, PrivilegePolicy, ResolveOptions,
        SplAccount, SplTransferLeg,
    },
};

const HOOK_ID: Pubkey = Pubkey::new_from_array([91; 32]);

struct World {
    context: ProgramTestContext,
    mint: Keypair,
    sender: Keypair,
    receiver: Keypair,
}

fn assert_arb_error(result: Result<(), BanksClientError>, expected: ArbError) {
    match result {
        Err(BanksClientError::TransactionError(TransactionError::InstructionError(
            _,
            InstructionError::Custom(code),
        ))) => assert_eq!(
            code,
            expected.code(),
            "expected {expected:?}, got {code:#x}"
        ),
        other => panic!("expected {expected:?}, got {other:?}"),
    }
}

async fn send(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
    extra: &[&Keypair],
) -> Result<(), BanksClientError> {
    let mut signers: Vec<&Keypair> = vec![&context.payer];
    signers.extend_from_slice(extra);
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        &signers,
        blockhash,
    );
    context.banks_client.process_transaction(tx).await
}

async fn token_amount(context: &mut ProgramTestContext, key: Pubkey) -> u64 {
    let account = context
        .banks_client
        .get_account(key)
        .await
        .unwrap()
        .expect("token account exists");
    StateWithExtensions::<TokenAccount>::unpack(&account.data)
        .unwrap()
        .base
        .amount
}

async fn stats_of(context: &mut ProgramTestContext, mint: &Pubkey) -> Stats {
    let key = stats_address(mint, &HOOK_ID).0;
    let account = context
        .banks_client
        .get_account(key)
        .await
        .unwrap()
        .unwrap();
    Stats::decode(&account.data).expect("decode stats")
}

/// A Token-2022 mint whose TransferHook points at the arbitrary hook, two funded accounts, and
/// the hook initialised with `max_per_slot`. Set `initialize` to false to test init separately.
async fn world(max_per_slot: u32, initialize: bool) -> World {
    let test = ProgramTest::new(
        "arbitrary_test_hook",
        HOOK_ID,
        processor!(process_instruction),
    );
    let mut context = test.start_with_context().await;
    let mint = Keypair::new();
    let sender = Keypair::new();
    let receiver = Keypair::new();
    let payer = context.payer.pubkey();
    let rent = context.banks_client.get_rent().await.unwrap();
    let mint_len =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook]).unwrap();
    let account_len = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
        ExtensionType::TransferHookAccount,
    ])
    .unwrap();
    let mut setup = vec![
        system_instruction::create_account(
            &payer,
            &mint.pubkey(),
            rent.minimum_balance(mint_len),
            mint_len as u64,
            &spl_token_2022::id(),
        ),
        transfer_hook_instruction::initialize(
            &spl_token_2022::id(),
            &mint.pubkey(),
            Some(payer),
            Some(HOOK_ID),
        )
        .unwrap(),
        token_instruction::initialize_mint2(&spl_token_2022::id(), &mint.pubkey(), &payer, None, 0)
            .unwrap(),
    ];
    for account in [&sender, &receiver] {
        setup.push(system_instruction::create_account(
            &payer,
            &account.pubkey(),
            rent.minimum_balance(account_len),
            account_len as u64,
            &spl_token_2022::id(),
        ));
        setup.push(
            token_instruction::initialize_account3(
                &spl_token_2022::id(),
                &account.pubkey(),
                &mint.pubkey(),
                &payer,
            )
            .unwrap(),
        );
    }
    setup.push(
        token_instruction::mint_to(
            &spl_token_2022::id(),
            &mint.pubkey(),
            &sender.pubkey(),
            &payer,
            &[],
            1_000,
        )
        .unwrap(),
    );
    if initialize {
        setup.push(init_instruction(
            HOOK_ID,
            payer,
            mint.pubkey(),
            payer,
            max_per_slot,
        ));
    }
    send(&mut context, &setup, &[&mint, &sender, &receiver])
        .await
        .expect("create mint, accounts and (optionally) initialise the hook");
    World {
        context,
        mint,
        sender,
        receiver,
    }
}

/// Resolve the transfer's hook accounts with the SDK, exactly as an integrator would.
async fn resolved_transfer(world: &mut World, amount: u64) -> Instruction {
    let payer = world.context.payer.pubkey();
    let mut fetched = HashMap::new();
    for key in [
        world.mint.pubkey(),
        validation_list_address(&world.mint.pubkey(), &HOOK_ID).0,
        HOOK_ID,
    ] {
        let account = world
            .context
            .banks_client
            .get_account(key)
            .await
            .unwrap()
            .expect("resolver account exists");
        fetched.insert(
            key,
            SplAccount {
                key,
                owner: account.owner,
                data: account.data,
                executable: account.executable,
            },
        );
    }
    // Natively, ProgramTest registers the hook under the native loader, which the SDK refuses by
    // default; allow it explicitly. The hook's own stats account is the one writable extra the
    // integrator accepts.
    let mut loaders = default_allowed_loaders();
    loaders.push(solana_sdk::native_loader::id());
    let options = ResolveOptions::default()
        .with_expected_hook_program(HOOK_ID)
        .with_allowed_loaders(loaders)
        .with_privilege_policy(PrivilegePolicy::allowing_writable([stats_address(
            &world.mint.pubkey(),
            &HOOK_ID,
        )
        .0]));
    let leg = resolve_leg(
        LegRole::Input,
        SplTransferLeg {
            source: world.sender.pubkey(),
            mint: world.mint.pubkey(),
            destination: world.receiver.pubkey(),
            authority: payer,
            amount,
        },
        &options,
        |key| {
            let account = fetched.get(&key).cloned();
            async move { Ok::<_, FetchError>(account) }
        },
    )
    .await
    .expect("resolve the arbitrary hook's accounts with the unchanged SDK");
    let metas = leg.slice().expect("hooked leg").metas();
    let mut transfer = token_instruction::transfer_checked(
        &spl_token_2022::id(),
        &world.sender.pubkey(),
        &world.mint.pubkey(),
        &world.receiver.pubkey(),
        &payer,
        &[],
        amount,
        0,
    )
    .unwrap();
    transfer.accounts.extend(metas.iter().cloned());
    transfer
}

#[tokio::test]
async fn init_creates_policy_stats_and_a_generic_validation_list() {
    let mut w = world(3, true).await;
    let mint = w.mint.pubkey();
    let policy = w
        .context
        .banks_client
        .get_account(policy_address(&mint, &HOOK_ID).0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(policy.owner, HOOK_ID);
    assert_eq!(policy.data.len(), POLICY_LEN);
    let decoded = Policy::decode(&policy.data).unwrap();
    assert_eq!((decoded.mint, decoded.max_per_slot), (mint, 3));
    let stats = w
        .context
        .banks_client
        .get_account(stats_address(&mint, &HOOK_ID).0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((stats.owner, stats.data.len()), (HOOK_ID, STATS_LEN));
    let list = w
        .context
        .banks_client
        .get_account(validation_list_address(&mint, &HOOK_ID).0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(list.owner, HOOK_ID);
    assert_eq!(&list.data[..8], &EXECUTE_DISCRIMINATOR);
    // Re-initialising is refused.
    let again = init_instruction(
        HOOK_ID,
        w.context.payer.pubkey(),
        mint,
        w.context.payer.pubkey(),
        9,
    );
    assert_arb_error(
        send(&mut w.context, &[again], &[]).await,
        ArbError::AlreadyInitialized,
    );
}

#[tokio::test]
async fn sdk_resolves_n_plus_two_accounts_without_knowing_the_hook() {
    let mut w = world(3, true).await;
    let transfer = resolved_transfer(&mut w, 5).await;
    let mint = w.mint.pubkey();
    let appended: Vec<AccountMeta> = transfer.accounts[4..].to_vec();
    // [policy, stats, hook program, validation list] = N + 2 with N = 2.
    assert_eq!(appended.len(), EXTRA_ACCOUNTS + 2);
    assert_eq!(appended[0].pubkey, policy_address(&mint, &HOOK_ID).0);
    assert!(!appended[0].is_writable);
    assert_eq!(appended[1].pubkey, stats_address(&mint, &HOOK_ID).0);
    assert!(appended[1].is_writable, "the stats meta must stay writable");
    assert_eq!(appended[2].pubkey, HOOK_ID);
    assert_eq!(
        appended[3].pubkey,
        validation_list_address(&mint, &HOOK_ID).0
    );
}

#[tokio::test]
async fn transfers_mutate_stats_and_the_over_limit_transfer_rolls_everything_back() {
    let mut w = world(2, true).await;
    let mint = w.mint.pubkey();
    let sender = w.sender.pubkey();
    let receiver = w.receiver.pubkey();

    // Two transfers in the same slot pass and mutate the stats account.
    for amount in [5u64, 7] {
        let transfer = resolved_transfer(&mut w, amount).await;
        send(&mut w.context, &[transfer], &[])
            .await
            .expect("transfer within the per-slot limit");
    }
    let stats = stats_of(&mut w.context, &mint).await;
    assert_eq!((stats.count, stats.total), (2, 12));
    assert_eq!(token_amount(&mut w.context, receiver).await, 12);

    // The third in the same slot is rejected by the hook with its own error code, and neither
    // the balances nor the hook's counter change.
    let transfer = resolved_transfer(&mut w, 9).await;
    assert_arb_error(
        send(&mut w.context, &[transfer], &[]).await,
        ArbError::SlotLimitExceeded,
    );
    assert_eq!(token_amount(&mut w.context, sender).await, 1_000 - 12);
    assert_eq!(token_amount(&mut w.context, receiver).await, 12);
    assert_eq!(stats_of(&mut w.context, &mint).await, stats);

    // A later slot resets the per-slot counter; the running total keeps accumulating.
    let slot = w.context.banks_client.get_root_slot().await.unwrap();
    w.context.warp_to_slot(slot + 20).unwrap();
    let transfer = resolved_transfer(&mut w, 11).await;
    send(&mut w.context, &[transfer], &[])
        .await
        .expect("a new slot starts a new count");
    let after = stats_of(&mut w.context, &mint).await;
    assert_eq!((after.count, after.total), (1, 23));
    assert!(after.slot > stats.slot);
}

#[tokio::test]
async fn direct_execute_is_rejected() {
    let mut w = world(3, true).await;
    let mint = w.mint.pubkey();
    let mut data = EXECUTE_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&1u64.to_le_bytes());
    let direct = Instruction {
        program_id: HOOK_ID,
        accounts: vec![
            AccountMeta::new_readonly(w.sender.pubkey(), false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(w.receiver.pubkey(), false),
            AccountMeta::new_readonly(w.context.payer.pubkey(), true),
            AccountMeta::new_readonly(validation_list_address(&mint, &HOOK_ID).0, false),
            AccountMeta::new_readonly(policy_address(&mint, &HOOK_ID).0, false),
            AccountMeta::new(stats_address(&mint, &HOOK_ID).0, false),
        ],
        data,
    };
    assert_arb_error(
        send(&mut w.context, &[direct], &[]).await,
        ArbError::NotDirectInvocation,
    );
    assert_eq!(stats_of(&mut w.context, &mint).await.count, 0);
}

#[tokio::test]
async fn init_requires_the_live_extension_authority_and_a_token_2022_mint() {
    let mut w = world(3, false).await;
    let payer = w.context.payer.pubkey();
    let mint = w.mint.pubkey();
    let impostor = Keypair::new();
    let wrong = init_instruction(HOOK_ID, payer, mint, impostor.pubkey(), 3);
    assert_arb_error(
        send(&mut w.context, &[wrong], &[&impostor]).await,
        ArbError::AuthorityMismatch,
    );
    let fake_mint = Pubkey::new_unique();
    let not_token_2022 = init_instruction(HOOK_ID, payer, fake_mint, payer, 3);
    assert_arb_error(
        send(&mut w.context, &[not_token_2022], &[]).await,
        ArbError::MintNotToken2022,
    );
    let zero = init_instruction(HOOK_ID, payer, mint, payer, 0);
    assert_arb_error(
        send(&mut w.context, &[zero], &[]).await,
        ArbError::InvalidParams,
    );
    // The legitimate init still works afterwards (nothing was half-created).
    let ok = init_instruction(HOOK_ID, payer, mint, payer, 3);
    send(&mut w.context, &[ok], &[]).await.expect("init");
}
