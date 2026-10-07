use {
    reference_hook_onchain::{
        config_address, initialize_hook_instruction, process_instruction, AuthorityMode, HookError,
        InitializeHookArgs,
    },
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        instruction::{AccountMeta, InstructionError},
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
};

const HOOK_PROGRAM_ID: Pubkey = Pubkey::new_from_array([47; 32]);

fn create_program_test() -> (ProgramTest, Keypair, Pubkey, Pubkey) {
    let mint = Keypair::new();
    let (config, _) = config_address(&mint.pubkey(), &HOOK_PROGRAM_ID);
    let validation_list = spl_transfer_hook_interface::get_extra_account_metas_address(
        &mint.pubkey(),
        &HOOK_PROGRAM_ID,
    );

    let test = ProgramTest::new(
        "reference_hook_onchain",
        HOOK_PROGRAM_ID,
        processor!(process_instruction),
    );
    (test, mint, config, validation_list)
}

fn assert_custom_error(
    result: Result<(), solana_program_test::BanksClientError>,
    expected: HookError,
) {
    match result {
        Err(solana_program_test::BanksClientError::TransactionError(
            TransactionError::InstructionError(_, InstructionError::Custom(code)),
        )) => assert_eq!(code, expected.code(), "expected {expected}"),
        other => panic!("expected {expected}, got {other:?}"),
    }
}

#[tokio::test]
async fn token_2022_transfer_executes_hook_and_rejection_rolls_back_balances() {
    let (test, mint, config, validation_list) = create_program_test();
    let source_account = Keypair::new();
    let destination_account = Keypair::new();
    let token_program = spl_token_2022::id();
    let mint_len =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook]).unwrap();
    let token_account_len = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
        ExtensionType::TransferHookAccount,
    ])
    .unwrap();

    let context = test.start_with_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();
    let mut setup_instructions = vec![
        system_instruction::create_account(
            &context.payer.pubkey(),
            &mint.pubkey(),
            rent.minimum_balance(mint_len),
            mint_len as u64,
            &token_program,
        ),
        transfer_hook_instruction::initialize(
            &token_program,
            &mint.pubkey(),
            Some(context.payer.pubkey()),
            Some(HOOK_PROGRAM_ID),
        )
        .unwrap(),
        token_instruction::initialize_mint2(
            &token_program,
            &mint.pubkey(),
            &context.payer.pubkey(),
            None,
            0,
        )
        .unwrap(),
        system_instruction::create_account(
            &context.payer.pubkey(),
            &source_account.pubkey(),
            rent.minimum_balance(token_account_len),
            token_account_len as u64,
            &token_program,
        ),
        token_instruction::initialize_account3(
            &token_program,
            &source_account.pubkey(),
            &mint.pubkey(),
            &context.payer.pubkey(),
        )
        .unwrap(),
        system_instruction::create_account(
            &context.payer.pubkey(),
            &destination_account.pubkey(),
            rent.minimum_balance(token_account_len),
            token_account_len as u64,
            &token_program,
        ),
        token_instruction::initialize_account3(
            &token_program,
            &destination_account.pubkey(),
            &mint.pubkey(),
            &context.payer.pubkey(),
        )
        .unwrap(),
    ];
    setup_instructions.push(initialize_hook_instruction(
        HOOK_PROGRAM_ID,
        mint.pubkey(),
        context.payer.pubkey(),
        context.payer.pubkey(),
        &InitializeHookArgs::max_transfer(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    ));
    setup_instructions.push(
        token_instruction::mint_to(
            &token_program,
            &mint.pubkey(),
            &source_account.pubkey(),
            &context.payer.pubkey(),
            &[],
            100,
        )
        .unwrap(),
    );
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let setup_tx = Transaction::new_signed_with_payer(
        &setup_instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer, &mint, &source_account, &destination_account],
        blockhash,
    );
    context
        .banks_client
        .process_transaction(setup_tx)
        .await
        .unwrap();
    let mint_state = context
        .banks_client
        .get_account(mint.pubkey())
        .await
        .unwrap()
        .unwrap();
    let mint_state = StateWithExtensions::<Mint>::unpack(&mint_state.data).unwrap();
    assert_eq!(
        spl_token_2022::extension::transfer_hook::get_program_id(&mint_state),
        Some(HOOK_PROGRAM_ID)
    );

    let direct_execute = spl_transfer_hook_interface::instruction::execute_with_extra_account_metas(
        &HOOK_PROGRAM_ID,
        &source_account.pubkey(),
        &mint.pubkey(),
        &destination_account.pubkey(),
        &context.payer.pubkey(),
        &validation_list,
        &[AccountMeta::new_readonly(config, false)],
        1,
    );
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let direct_call_tx = Transaction::new_signed_with_payer(
        &[direct_execute],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    assert_custom_error(
        context
            .banks_client
            .process_transaction(direct_call_tx)
            .await,
        HookError::NotDirectInvocation,
    );

    let transfer = |amount| {
        let mut instruction = token_instruction::transfer_checked(
            &token_program,
            &source_account.pubkey(),
            &mint.pubkey(),
            &destination_account.pubkey(),
            &context.payer.pubkey(),
            &[],
            amount,
            0,
        )
        .unwrap();
        instruction.accounts.extend([
            AccountMeta::new_readonly(config, false),
            AccountMeta::new_readonly(HOOK_PROGRAM_ID, false),
            AccountMeta::new_readonly(validation_list, false),
        ]);
        assert_eq!(instruction.accounts.len(), 7);
        instruction
    };
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let successful_transfer = Transaction::new_signed_with_payer(
        &[transfer(40)],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    context
        .banks_client
        .process_transaction(successful_transfer)
        .await
        .unwrap();

    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let rejected_transfer = Transaction::new_signed_with_payer(
        &[transfer(60)],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    assert_custom_error(
        context
            .banks_client
            .process_transaction(rejected_transfer)
            .await,
        HookError::TransferExceedsLimit,
    );

    let source_after = context
        .banks_client
        .get_account(source_account.pubkey())
        .await
        .unwrap()
        .unwrap();
    let destination_after = context
        .banks_client
        .get_account(destination_account.pubkey())
        .await
        .unwrap()
        .unwrap();
    let source_state = StateWithExtensions::<TokenAccount>::unpack(&source_after.data).unwrap();
    let destination_state =
        StateWithExtensions::<TokenAccount>::unpack(&destination_after.data).unwrap();
    assert_eq!(source_state.base.amount, 60);
    assert_eq!(destination_state.base.amount, 40);
}
