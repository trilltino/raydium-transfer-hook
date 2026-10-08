use {
    solana_program::{
        hash::hash,
        instruction::{AccountMeta, Instruction},
        program_option::COption,
        program_pack::Pack,
        pubkey::Pubkey,
        system_instruction, sysvar,
    },
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account::Account,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
    spl_token_2022::{
        extension::{
            transfer_hook::instruction as transfer_hook_instruction, ExtensionType,
            StateWithExtensions,
        },
        instruction as token_2022_instruction,
        state::{Account as TokenAccount, Mint},
    },
    std::collections::HashMap,
    transfer_hook_sdk::{
        frame_cpmm_swap_base_input_v2, resolve_leg, FetchError, LegHook, LegRole, ResolveOptions,
        SplAccount, SplTransferLeg,
    },
    transfer_hook_starter::{
        config_address, initialize_hook_instruction, process_instruction, AuthorityMode, HookError,
        InitializeHookArgs,
    },
};

const CPMM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
const HOOK_PROGRAM_ID: Pubkey = Pubkey::new_from_array([47; 32]);
const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const POOL_SEED: &[u8] = b"pool";
const AMM_CONFIG_SEED: &[u8] = b"amm_config";
const AUTH_SEED: &[u8] = b"vault_and_lp_mint_auth_seed";
const POOL_VAULT_SEED: &[u8] = b"pool_vault";
const POOL_LP_MINT_SEED: &[u8] = b"pool_lp_mint";
const OBSERVATION_SEED: &[u8] = b"observation";
const SUPPORT_MINT_SEED: &[u8] = b"support_mint";
const CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
const INPUT_HOOK_LIMIT: u64 = 500;
const OUTPUT_HOOK_LIMIT: u64 = 20;
const INITIAL_LIQUIDITY: u64 = 1_000_000;

struct PoolFixture {
    amm_config: Pubkey,
    authority: Pubkey,
    pool_state: Pubkey,
    input_mint: Keypair,
    output_mint: Keypair,
    input_vault: Pubkey,
    output_vault: Pubkey,
    observation_state: Pubkey,
    trader_input: Keypair,
    trader_output: Keypair,
}

fn discriminator(namespace: &str) -> [u8; 8] {
    hash(namespace.as_bytes()).to_bytes()[..8]
        .try_into()
        .expect("Anchor discriminator is eight bytes")
}

fn anchor_account_data(name: &str, fields: &[u8]) -> Vec<u8> {
    let mut data = discriminator(&format!("account:{name}")).to_vec();
    data.extend_from_slice(fields);
    data
}

fn amm_config_account(rent: u64) -> Account {
    let (_, bump) =
        Pubkey::find_program_address(&[AMM_CONFIG_SEED, &0u16.to_be_bytes()], &CPMM_PROGRAM_ID);
    let mut fields = Vec::with_capacity(228);
    fields.push(bump);
    fields.push(0);
    fields.extend_from_slice(&0u16.to_le_bytes());
    for value in [0u64; 4] {
        fields.extend_from_slice(&value.to_le_bytes());
    }
    fields.extend_from_slice(Pubkey::default().as_ref());
    fields.extend_from_slice(Pubkey::default().as_ref());
    fields.extend_from_slice(&0u64.to_le_bytes());
    fields.extend_from_slice(&0u64.to_le_bytes());
    fields.extend_from_slice(&[0; 14 * 8]);
    assert_eq!(fields.len() + 8, 236);
    Account {
        lamports: rent,
        data: anchor_account_data("AmmConfig", &fields),
        owner: CPMM_PROGRAM_ID,
        executable: false,
        rent_epoch: 0,
    }
}

fn support_mint_account(mint: Pubkey, rent: u64) -> Account {
    let (_, bump) =
        Pubkey::find_program_address(&[SUPPORT_MINT_SEED, mint.as_ref()], &CPMM_PROGRAM_ID);
    let mut fields = Vec::with_capacity(97);
    fields.push(bump);
    fields.extend_from_slice(mint.as_ref());
    fields.extend_from_slice(&[0; 8 * 8]);
    Account {
        lamports: rent,
        data: anchor_account_data("SupportMintAssociated", &fields),
        owner: CPMM_PROGRAM_ID,
        executable: false,
        rent_epoch: 0,
    }
}

fn create_pool_fee_account() -> Account {
    let mut data = vec![0; spl_token::state::Account::LEN];
    spl_token::state::Account::pack(
        spl_token::state::Account {
            mint: spl_token::native_mint::id(),
            owner: Pubkey::default(),
            amount: 0,
            delegate: COption::None,
            state: spl_token::state::AccountState::Initialized,
            is_native: COption::Some(0),
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut data,
    )
    .expect("pack the zero-fee receiver token account");
    Account {
        lamports: 1,
        data,
        owner: spl_token::id(),
        executable: false,
        rent_epoch: 0,
    }
}

fn token_account_len() -> usize {
    ExtensionType::try_calculate_account_len::<TokenAccount>(&[ExtensionType::TransferHookAccount])
        .expect("calculate Token-2022 account size")
}

fn token_account_create_instructions(
    payer: Pubkey,
    account: Pubkey,
    mint: Pubkey,
    owner: Pubkey,
    rent: &solana_sdk::rent::Rent,
) -> Vec<Instruction> {
    let len = token_account_len();
    vec![
        system_instruction::create_account(
            &payer,
            &account,
            rent.minimum_balance(len),
            len as u64,
            &spl_token_2022::id(),
        ),
        token_2022_instruction::initialize_account3(&spl_token_2022::id(), &account, &mint, &owner)
            .expect("build Token-2022 account initialization"),
    ]
}

fn pool_initialize_instruction(fixture: &PoolFixture, payer: Pubkey) -> Instruction {
    let config = fixture.amm_config;
    let mint_0 = fixture.input_mint.pubkey();
    let mint_1 = fixture.output_mint.pubkey();
    let (lp_mint, _) = Pubkey::find_program_address(
        &[POOL_LP_MINT_SEED, fixture.pool_state.as_ref()],
        &CPMM_PROGRAM_ID,
    );
    let lp_token = Pubkey::find_program_address(
        &[payer.as_ref(), spl_token::id().as_ref(), lp_mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0;
    let (support_0, _) =
        Pubkey::find_program_address(&[SUPPORT_MINT_SEED, mint_0.as_ref()], &CPMM_PROGRAM_ID);
    let (support_1, _) =
        Pubkey::find_program_address(&[SUPPORT_MINT_SEED, mint_1.as_ref()], &CPMM_PROGRAM_ID);
    let mut data = discriminator("global:initialize").to_vec();
    data.extend_from_slice(&INITIAL_LIQUIDITY.to_le_bytes());
    data.extend_from_slice(&INITIAL_LIQUIDITY.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());

    let mut accounts = vec![
        AccountMeta::new(payer, true),
        AccountMeta::new_readonly(config, false),
        AccountMeta::new_readonly(fixture.authority, false),
        AccountMeta::new(fixture.pool_state, false),
        AccountMeta::new_readonly(mint_0, false),
        AccountMeta::new_readonly(mint_1, false),
        AccountMeta::new(lp_mint, false),
        AccountMeta::new(fixture.trader_input.pubkey(), false),
        AccountMeta::new(fixture.trader_output.pubkey(), false),
        AccountMeta::new(lp_token, false),
        AccountMeta::new(fixture.input_vault, false),
        AccountMeta::new(fixture.output_vault, false),
        AccountMeta::new(raydium_create_pool_fee_receiver(), false),
        AccountMeta::new(fixture.observation_state, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(spl_token_2022::id(), false),
        AccountMeta::new_readonly(spl_token_2022::id(), false),
        AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
        AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        AccountMeta::new_readonly(sysvar::rent::id(), false),
    ];
    accounts.push(AccountMeta::new_readonly(support_0, false));
    accounts.push(AccountMeta::new_readonly(support_1, false));
    Instruction {
        program_id: CPMM_PROGRAM_ID,
        accounts,
        data,
    }
}

fn raydium_create_pool_fee_receiver() -> Pubkey {
    solana_sdk::pubkey!("DNXgeM9EiiaAbaWvwjHj9fQQLAX5ZsfHyvmYUNRAdNC8")
}

fn observation_address(pool_state: Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[OBSERVATION_SEED, pool_state.as_ref()], &CPMM_PROGRAM_ID).0
}

fn swap_instruction(
    fixture: &PoolFixture,
    payer: Pubkey,
    amount_in: u64,
    minimum_amount_out: u64,
) -> Instruction {
    let mut data = CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&minimum_amount_out.to_le_bytes());
    Instruction {
        program_id: CPMM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(fixture.authority, false),
            AccountMeta::new_readonly(fixture.amm_config, false),
            AccountMeta::new(fixture.pool_state, false),
            AccountMeta::new(fixture.trader_input.pubkey(), false),
            AccountMeta::new(fixture.trader_output.pubkey(), false),
            AccountMeta::new(fixture.input_vault, false),
            AccountMeta::new(fixture.output_vault, false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(fixture.input_mint.pubkey(), false),
            AccountMeta::new_readonly(fixture.output_mint.pubkey(), false),
            AccountMeta::new(fixture.observation_state, false),
        ],
        data,
    }
}

async fn fetch_resolver_accounts(
    context: &mut solana_program_test::ProgramTestContext,
    fixture: &PoolFixture,
) -> HashMap<Pubkey, SplAccount> {
    let mut accounts = HashMap::new();
    for mint in [&fixture.input_mint, &fixture.output_mint] {
        for address in [
            mint.pubkey(),
            config_address(&mint.pubkey(), &HOOK_PROGRAM_ID).0,
            spl_transfer_hook_interface::get_extra_account_metas_address(
                &mint.pubkey(),
                &HOOK_PROGRAM_ID,
            ),
        ] {
            let account = context
                .banks_client
                .get_account(address)
                .await
                .expect("fetch hook resolver account")
                .expect("hook resolver account must exist");
            accounts.insert(
                address,
                SplAccount {
                    key: address,
                    owner: account.owner,
                    data: account.data,
                    executable: account.executable,
                },
            );
        }
    }
    let hook_program = context
        .banks_client
        .get_account(HOOK_PROGRAM_ID)
        .await
        .expect("fetch reference hook program")
        .expect("reference hook program must be registered");
    accounts.insert(
        HOOK_PROGRAM_ID,
        SplAccount {
            key: HOOK_PROGRAM_ID,
            owner: hook_program.owner,
            data: hook_program.data,
            executable: hook_program.executable,
        },
    );
    accounts
}

async fn resolve_hook_slice(
    role: LegRole,
    source: Pubkey,
    mint: Pubkey,
    destination: Pubkey,
    authority: Pubkey,
    amount: u64,
    fetched: &HashMap<Pubkey, SplAccount>,
) -> LegHook {
    // Pin the hook program: the mint must use exactly the reference hook.
    let options = ResolveOptions::default().with_expected_hook_program(HOOK_PROGRAM_ID);
    let leg = resolve_leg(
        role,
        SplTransferLeg {
            source,
            mint,
            destination,
            authority,
            amount,
        },
        &options,
        |key| {
            let account = fetched.get(&key).cloned();
            async move { Ok::<_, FetchError>(account) }
        },
    )
    .await
    .expect("resolve SPL Transfer Hook accounts");
    let slice = leg
        .slice()
        .expect("Token-2022 swap leg must have a Transfer Hook");
    let appended = slice.metas();
    assert_eq!(appended.len(), 3);
    assert_eq!(
        appended[0].pubkey,
        config_address(&mint, &HOOK_PROGRAM_ID).0
    );
    assert_eq!(appended[1].pubkey, HOOK_PROGRAM_ID);
    assert_eq!(
        appended[2].pubkey,
        spl_transfer_hook_interface::get_extra_account_metas_address(&mint, &HOOK_PROGRAM_ID)
    );
    leg
}

async fn framed_swap(
    context: &mut solana_program_test::ProgramTestContext,
    fixture: &PoolFixture,
    payer: Pubkey,
    amount_in: u64,
    expected_amount_out: u64,
) -> Instruction {
    let fetched = fetch_resolver_accounts(context, fixture).await;
    let input_leg = resolve_hook_slice(
        LegRole::Input,
        fixture.trader_input.pubkey(),
        fixture.input_mint.pubkey(),
        fixture.input_vault,
        payer,
        amount_in,
        &fetched,
    )
    .await;
    let output_leg = resolve_hook_slice(
        LegRole::Output,
        fixture.output_vault,
        fixture.output_mint.pubkey(),
        fixture.trader_output.pubkey(),
        fixture.authority,
        expected_amount_out,
        &fetched,
    )
    .await;
    assert_ne!(input_leg.slice(), output_leg.slice());
    let mut instruction = swap_instruction(fixture, payer, amount_in, 1);
    frame_cpmm_swap_base_input_v2(&mut instruction, &input_leg, &output_leg)
        .expect("frame the two independent hook slices");
    instruction
}

#[tokio::test]
#[ignore = "requires SBF_OUT_DIR=target/localnet-sbf after `cargo xtask localnet build`"]
async fn cpmm_sbf_v2_executes_both_token_2022_hook_legs_and_rolls_back_output_rejection() {
    let mut input_mint = Keypair::new();
    let mut output_mint = Keypair::new();
    if input_mint.pubkey() > output_mint.pubkey() {
        std::mem::swap(&mut input_mint, &mut output_mint);
    }
    let trader_input = Keypair::new();
    let trader_output = Keypair::new();
    let provider_input = Keypair::new();
    let provider_output = Keypair::new();
    let (amm_config, _) =
        Pubkey::find_program_address(&[AMM_CONFIG_SEED, &0u16.to_be_bytes()], &CPMM_PROGRAM_ID);
    let (authority, _) = Pubkey::find_program_address(&[AUTH_SEED], &CPMM_PROGRAM_ID);
    let (pool_state, _) = Pubkey::find_program_address(
        &[
            POOL_SEED,
            amm_config.as_ref(),
            input_mint.pubkey().as_ref(),
            output_mint.pubkey().as_ref(),
        ],
        &CPMM_PROGRAM_ID,
    );
    let input_vault = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED,
            pool_state.as_ref(),
            input_mint.pubkey().as_ref(),
        ],
        &CPMM_PROGRAM_ID,
    )
    .0;
    let output_vault = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED,
            pool_state.as_ref(),
            output_mint.pubkey().as_ref(),
        ],
        &CPMM_PROGRAM_ID,
    )
    .0;
    let fixture = PoolFixture {
        amm_config,
        authority,
        pool_state,
        input_mint,
        output_mint,
        input_vault,
        output_vault,
        observation_state: observation_address(pool_state),
        trader_input,
        trader_output,
    };

    let mut program_test = ProgramTest::new(
        "transfer_hook_starter",
        HOOK_PROGRAM_ID,
        processor!(process_instruction),
    );
    program_test.add_program("raydium_cp_swap", CPMM_PROGRAM_ID, None);

    let rent = solana_sdk::rent::Rent::default();
    program_test.add_account(
        fixture.amm_config,
        amm_config_account(rent.minimum_balance(236)),
    );
    for mint in [&fixture.input_mint, &fixture.output_mint] {
        let support_key = Pubkey::find_program_address(
            &[SUPPORT_MINT_SEED, mint.pubkey().as_ref()],
            &CPMM_PROGRAM_ID,
        )
        .0;
        program_test.add_account(
            support_key,
            support_mint_account(mint.pubkey(), rent.minimum_balance(105)),
        );
    }
    program_test.add_account(
        raydium_create_pool_fee_receiver(),
        create_pool_fee_account(),
    );
    let mut context = program_test.start_with_context().await;

    let mint_len = ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook])
        .expect("calculate Token-2022 mint size");
    let context_rent = context.banks_client.get_rent().await.unwrap();
    let mut create_instructions = Vec::new();
    for mint in [&fixture.input_mint, &fixture.output_mint] {
        create_instructions.push(system_instruction::create_account(
            &context.payer.pubkey(),
            &mint.pubkey(),
            context_rent.minimum_balance(mint_len),
            mint_len as u64,
            &spl_token_2022::id(),
        ));
        create_instructions.push(
            transfer_hook_instruction::initialize(
                &spl_token_2022::id(),
                &mint.pubkey(),
                Some(context.payer.pubkey()),
                None,
            )
            .unwrap(),
        );
        create_instructions.push(
            token_2022_instruction::initialize_mint2(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &context.payer.pubkey(),
                None,
                0,
            )
            .unwrap(),
        );
    }
    for (account, mint) in [
        (&provider_input, &fixture.input_mint),
        (&provider_output, &fixture.output_mint),
        (&fixture.trader_input, &fixture.input_mint),
        (&fixture.trader_output, &fixture.output_mint),
    ] {
        create_instructions.extend(token_account_create_instructions(
            context.payer.pubkey(),
            account.pubkey(),
            mint.pubkey(),
            context.payer.pubkey(),
            &context_rent,
        ));
    }
    create_instructions.push(
        token_2022_instruction::mint_to(
            &spl_token_2022::id(),
            &fixture.input_mint.pubkey(),
            &provider_input.pubkey(),
            &context.payer.pubkey(),
            &[],
            INITIAL_LIQUIDITY * 2,
        )
        .unwrap(),
    );
    create_instructions.push(
        token_2022_instruction::mint_to(
            &spl_token_2022::id(),
            &fixture.output_mint.pubkey(),
            &provider_output.pubkey(),
            &context.payer.pubkey(),
            &[],
            INITIAL_LIQUIDITY * 2,
        )
        .unwrap(),
    );
    create_instructions.push(
        token_2022_instruction::mint_to(
            &spl_token_2022::id(),
            &fixture.input_mint.pubkey(),
            &fixture.trader_input.pubkey(),
            &context.payer.pubkey(),
            &[],
            1_000,
        )
        .unwrap(),
    );
    let setup_tx = Transaction::new_signed_with_payer(
        &create_instructions,
        Some(&context.payer.pubkey()),
        &[
            &context.payer,
            &fixture.input_mint,
            &fixture.output_mint,
            &provider_input,
            &provider_output,
            &fixture.trader_input,
            &fixture.trader_output,
        ],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(setup_tx)
        .await
        .expect("create Token-2022 hook mints and funded accounts");
    let (lp_mint, _) = Pubkey::find_program_address(
        &[POOL_LP_MINT_SEED, fixture.pool_state.as_ref()],
        &CPMM_PROGRAM_ID,
    );
    let (input_support, _) = Pubkey::find_program_address(
        &[SUPPORT_MINT_SEED, fixture.input_mint.pubkey().as_ref()],
        &CPMM_PROGRAM_ID,
    );
    let (output_support, _) = Pubkey::find_program_address(
        &[SUPPORT_MINT_SEED, fixture.output_mint.pubkey().as_ref()],
        &CPMM_PROGRAM_ID,
    );
    let mut initialize_pool = pool_initialize_instruction(&fixture, context.payer.pubkey());
    initialize_pool.accounts[7].pubkey = provider_input.pubkey();
    initialize_pool.accounts[8].pubkey = provider_output.pubkey();
    assert_eq!(initialize_pool.accounts[20].pubkey, input_support);
    assert_eq!(initialize_pool.accounts[21].pubkey, output_support);
    let initialize_pool_tx = Transaction::new_signed_with_payer(
        &[initialize_pool],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(initialize_pool_tx)
        .await
        .expect("initialize a real CPMM pool with Token-2022 liquidity");

    let mut enable_hook_instructions = Vec::new();
    for (mint, limit) in [
        (&fixture.input_mint, INPUT_HOOK_LIMIT),
        (&fixture.output_mint, OUTPUT_HOOK_LIMIT),
    ] {
        enable_hook_instructions.push(
            transfer_hook_instruction::update(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &context.payer.pubkey(),
                &[],
                Some(HOOK_PROGRAM_ID),
            )
            .unwrap(),
        );
        enable_hook_instructions.push(initialize_hook_instruction(
            HOOK_PROGRAM_ID,
            mint.pubkey(),
            context.payer.pubkey(),
            context.payer.pubkey(),
            &InitializeHookArgs::max_transfer(
                AuthorityMode::ExtensionAuthority,
                limit,
                Pubkey::default(),
            ),
        ));
    }
    let enable_hook_tx = Transaction::new_signed_with_payer(
        &enable_hook_instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(enable_hook_tx)
        .await
        .expect("activate both hook mints and initialize their extra-meta lists");

    let mut clock: solana_sdk::clock::Clock = context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp += 10;
    context.set_sysvar(&clock);

    let pool_data = context
        .banks_client
        .get_account(fixture.pool_state)
        .await
        .unwrap()
        .expect("initialized CPMM pool exists");
    assert_eq!(pool_data.owner, CPMM_PROGRAM_ID);
    let input_mint_data = context
        .banks_client
        .get_account(fixture.input_mint.pubkey())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        spl_token_2022::extension::transfer_hook::get_program_id(
            &StateWithExtensions::<Mint>::unpack(&input_mint_data.data).unwrap()
        ),
        Some(HOOK_PROGRAM_ID)
    );
    assert!(context
        .banks_client
        .get_account(lp_mint)
        .await
        .unwrap()
        .is_some());

    let payer = context.payer.pubkey();
    assert_eq!(
        token_amount(&mut context, fixture.input_vault).await,
        INITIAL_LIQUIDITY
    );
    assert_eq!(
        token_amount(&mut context, fixture.output_vault).await,
        INITIAL_LIQUIDITY
    );
    assert_eq!(
        token_amount(&mut context, fixture.trader_input.pubkey()).await,
        1_000
    );
    let successful_swap = framed_swap(&mut context, &fixture, payer, 10, 9).await;
    let successful_tx = Transaction::new_signed_with_payer(
        &[successful_swap],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    let trace = trace_hook(&mut context, successful_tx.clone()).await;
    assert!(trace.succeeded, "simulated swap must succeed");
    assert_eq!(
        trace.hook_invocations, 2,
        "hook Execute must run once per hooked transfer leg (input then output)"
    );
    assert!(!trace.hook_failed_with_custom_1);
    context
        .banks_client
        .process_transaction(successful_tx)
        .await
        .expect("CPMM SBF swap should run both independent Transfer Hook CPIs");
    let input_after_success = token_amount(&mut context, fixture.trader_input.pubkey()).await;
    let output_after_success = token_amount(&mut context, fixture.trader_output.pubkey()).await;
    let input_vault_after_success = token_amount(&mut context, fixture.input_vault).await;
    let output_vault_after_success = token_amount(&mut context, fixture.output_vault).await;
    assert_eq!(input_after_success, 990);
    assert_eq!(output_after_success, 9);
    assert_eq!(input_vault_after_success, INITIAL_LIQUIDITY + 10);
    assert_eq!(output_vault_after_success, INITIAL_LIQUIDITY - 9);

    let rejected_swap = framed_swap(&mut context, &fixture, payer, 100, 99).await;
    let rejected_tx = Transaction::new_signed_with_payer(
        &[rejected_swap],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    let trace = trace_hook(&mut context, rejected_tx.clone()).await;
    assert!(!trace.succeeded);
    assert_eq!(
        trace.hook_invocations, 2,
        "input leg passes its hook, output leg reaches its hook"
    );
    assert!(
        trace.hook_failed_with_custom_1,
        "rejection must originate in the hook program, not elsewhere"
    );
    assert!(context
        .banks_client
        .process_transaction(rejected_tx)
        .await
        .is_err());
    assert_eq!(
        token_amount(&mut context, fixture.trader_input.pubkey()).await,
        input_after_success
    );
    assert_eq!(
        token_amount(&mut context, fixture.trader_output.pubkey()).await,
        output_after_success
    );
    assert_eq!(
        token_amount(&mut context, fixture.input_vault).await,
        input_vault_after_success
    );
    assert_eq!(
        token_amount(&mut context, fixture.output_vault).await,
        output_vault_after_success
    );

    let input_rejected_swap = framed_swap(&mut context, &fixture, payer, 600, 1).await;
    let input_rejected_tx = Transaction::new_signed_with_payer(
        &[input_rejected_swap],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    let trace = trace_hook(&mut context, input_rejected_tx.clone()).await;
    assert!(!trace.succeeded);
    assert_eq!(
        trace.hook_invocations, 1,
        "input-leg rejection aborts before the output leg hook runs"
    );
    assert!(trace.hook_failed_with_custom_1);
    assert!(context
        .banks_client
        .process_transaction(input_rejected_tx)
        .await
        .is_err());
    assert_eq!(
        token_amount(&mut context, fixture.trader_input.pubkey()).await,
        input_after_success
    );
    assert_eq!(
        token_amount(&mut context, fixture.trader_output.pubkey()).await,
        output_after_success
    );
    assert_eq!(
        token_amount(&mut context, fixture.input_vault).await,
        input_vault_after_success
    );
    assert_eq!(
        token_amount(&mut context, fixture.output_vault).await,
        output_vault_after_success
    );
}

async fn token_amount(
    context: &mut solana_program_test::ProgramTestContext,
    account: Pubkey,
) -> u64 {
    let account = context
        .banks_client
        .get_account(account)
        .await
        .expect("read token account")
        .expect("token account must exist");
    StateWithExtensions::<TokenAccount>::unpack(&account.data)
        .expect("unpack Token-2022 account")
        .base
        .amount
}

struct HookTrace {
    succeeded: bool,
    hook_invocations: usize,
    hook_failed_with_custom_1: bool,
}

/// Simulates the transaction (no state change) and reads the hook program's invocations out of
/// the runtime logs, so tests prove which hook legs executed and that a failure originated there.
async fn trace_hook(
    context: &mut solana_program_test::ProgramTestContext,
    transaction: Transaction,
) -> HookTrace {
    let outcome = context
        .banks_client
        .simulate_transaction(transaction)
        .await
        .expect("simulation request");
    let details = outcome.simulation_details.expect("simulation details");
    let invoke = format!("Program {HOOK_PROGRAM_ID} invoke");
    let failed = format!(
        "Program {HOOK_PROGRAM_ID} failed: custom program error: {:#x}",
        HookError::TransferExceedsLimit.code()
    );
    HookTrace {
        succeeded: outcome.result.expect("simulation result").is_ok(),
        hook_invocations: details
            .logs
            .iter()
            .filter(|l| l.starts_with(&invoke))
            .count(),
        hook_failed_with_custom_1: details.logs.iter().any(|l| l == &failed),
    }
}
