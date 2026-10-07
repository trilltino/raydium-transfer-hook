//! Runs the hook-aware CLMM `swap_v3` against the real `raydium_clmm` SBF program, the real
//! Token-2022 processor and the reference hook. Needs `raydium_clmm.so` (built from the pinned
//! hook-support revision in `upstream.lock.toml`) and `reference_hook_onchain.so` in
//! `SBF_OUT_DIR`.
//!
//! Real: pool creation, observation/bitmap accounts, tick arrays, an NFT-backed liquidity
//! position, the swap math, the Token-2022 CPIs and the hook CPIs. Injected (cannot be created
//! without Raydium's admin key): the `AmmConfig` account and one `SupportMintAssociated` account
//! per mint, which the program requires for any Token-2022 mint with a TransferHook extension.

use {
    reference_hook_onchain::{
        initialize_hook_instruction, process_instruction, AuthorityMode, HookError,
        InitializeHookArgs,
    },
    solana_program::{
        hash::hash,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        system_instruction, sysvar,
    },
    solana_program_test::{processor, ProgramTest, ProgramTestContext},
    solana_sdk::{
        account::Account,
        compute_budget::ComputeBudgetInstruction,
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
        build_clmm_swap_v2, frame_clmm_swap_v3, resolve_leg, ClmmSwapAccounts, ClmmSwapArgs,
        FetchError, LegHook, LegRole, ResolveOptions, SplAccount, SplTransferLeg,
    },
};

const CLMM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK");
const HOOK_PROGRAM_ID: Pubkey = Pubkey::new_from_array([47; 32]);
const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const MEMO_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
const PROTOCOL_OWNER: Pubkey = solana_sdk::pubkey!("projjosVCPQH49d5em7VYS7fJZzaqKixqKtus7yk416");
const FUND_OWNER: Pubkey = solana_sdk::pubkey!("FundHfY8oo8J9KYGyfXFFuQCHe7Z1VBNmsj84eMcdYs4");

const AMM_CONFIG_SEED: &[u8] = b"amm_config";
const POOL_SEED: &[u8] = b"pool";
const POOL_VAULT_SEED: &[u8] = b"pool_vault";
const OBSERVATION_SEED: &[u8] = b"observation";
const BITMAP_SEED: &[u8] = b"pool_tick_array_bitmap_extension";
const TICK_ARRAY_SEED: &[u8] = b"tick_array";
const POSITION_SEED: &[u8] = b"position";
const SUPPORT_MINT_SEED: &[u8] = b"support_mint";

const TICK_SPACING: u16 = 10;
const TRADE_FEE_RATE: u32 = 2_500;
const PROTOCOL_FEE_RATE: u32 = 120_000;
const TICK_LOWER: i32 = -300;
const TICK_UPPER: i32 = 300;
const LOWER_ARRAY_START: i32 = -600;
const UPPER_ARRAY_START: i32 = 0;
const LIQUIDITY: u128 = 100_000_000_000;
const DEPOSIT_MAX: u64 = 2_000_000_000;
const INPUT_HOOK_LIMIT: u64 = 500;
const OUTPUT_HOOK_LIMIT: u64 = 20;

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
        Pubkey::find_program_address(&[AMM_CONFIG_SEED, &0u16.to_be_bytes()], &CLMM_PROGRAM_ID);
    let mut fields = Vec::with_capacity(109);
    fields.push(bump);
    fields.extend_from_slice(&0u16.to_le_bytes());
    fields.extend_from_slice(PROTOCOL_OWNER.as_ref());
    fields.extend_from_slice(&PROTOCOL_FEE_RATE.to_le_bytes());
    fields.extend_from_slice(&TRADE_FEE_RATE.to_le_bytes());
    fields.extend_from_slice(&TICK_SPACING.to_le_bytes());
    fields.extend_from_slice(&0u32.to_le_bytes()); // fund_fee_rate
    fields.extend_from_slice(&0u32.to_le_bytes()); // padding_u32
    fields.extend_from_slice(FUND_OWNER.as_ref());
    fields.extend_from_slice(&[0; 24]); // padding [u64; 3]
    assert_eq!(fields.len() + 8, 117, "AmmConfig::LEN");
    Account {
        lamports: rent,
        data: anchor_account_data("AmmConfig", &fields),
        owner: CLMM_PROGRAM_ID,
        executable: false,
        rent_epoch: 0,
    }
}

fn support_mint_account(mint: Pubkey, rent: u64) -> Account {
    let (_, bump) =
        Pubkey::find_program_address(&[SUPPORT_MINT_SEED, mint.as_ref()], &CLMM_PROGRAM_ID);
    let mut fields = Vec::with_capacity(97);
    fields.push(bump);
    fields.extend_from_slice(mint.as_ref());
    fields.extend_from_slice(&[0; 64]);
    assert_eq!(fields.len() + 8, 105, "SupportMintAssociated::LEN");
    Account {
        lamports: rent,
        data: anchor_account_data("SupportMintAssociated", &fields),
        owner: CLMM_PROGRAM_ID,
        executable: false,
        rent_epoch: 0,
    }
}

fn tick_array_address(pool: &Pubkey, start_index: i32) -> Pubkey {
    Pubkey::find_program_address(
        &[TICK_ARRAY_SEED, pool.as_ref(), &start_index.to_be_bytes()],
        &CLMM_PROGRAM_ID,
    )
    .0
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

fn with_budget(instruction: Instruction) -> [Instruction; 2] {
    [
        ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
        instruction,
    ]
}

async fn send(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
    extra_signers: &[&Keypair],
) -> Result<(), solana_program_test::BanksClientError> {
    let mut signers: Vec<&Keypair> = vec![&context.payer];
    signers.extend_from_slice(extra_signers);
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        &signers,
        blockhash,
    );
    context.banks_client.process_transaction(tx).await
}

async fn token_amount(context: &mut ProgramTestContext, account: Pubkey) -> u64 {
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

async fn raw_account_data(context: &mut ProgramTestContext, key: Pubkey) -> Vec<u8> {
    context
        .banks_client
        .get_account(key)
        .await
        .expect("read account")
        .expect("account must exist")
        .data
}

struct HookTrace {
    succeeded: bool,
    hook_invocations: usize,
    hook_rejected_over_limit: bool,
}

/// Simulates the transaction (no state change) and reads the hook program's invocations out of
/// the runtime logs, so the test proves which hook legs executed and that a failure originated
/// in the hook program with the expected code.
async fn trace_hook(context: &mut ProgramTestContext, transaction: Transaction) -> HookTrace {
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
        hook_rejected_over_limit: details.logs.iter().any(|l| l == &failed),
    }
}

struct Fixture {
    amm_config: Pubkey,
    pool_state: Pubkey,
    mint_0: Keypair,
    mint_1: Keypair,
    vault_0: Pubkey,
    vault_1: Pubkey,
    observation: Pubkey,
    bitmap: Pubkey,
    trader_in: Keypair,
    trader_out: Keypair,
    tick_arrays: [Pubkey; 2],
}

async fn fetch_resolver_accounts(
    context: &mut ProgramTestContext,
    fixture: &Fixture,
) -> HashMap<Pubkey, SplAccount> {
    let mut accounts = HashMap::new();
    let mut keys = vec![HOOK_PROGRAM_ID];
    for mint in [&fixture.mint_0, &fixture.mint_1] {
        keys.push(mint.pubkey());
        keys.push(reference_hook_onchain::config_address(&mint.pubkey(), &HOOK_PROGRAM_ID).0);
        keys.push(
            spl_transfer_hook_interface::get_extra_account_metas_address(
                &mint.pubkey(),
                &HOOK_PROGRAM_ID,
            ),
        );
    }
    for key in keys {
        let account = context
            .banks_client
            .get_account(key)
            .await
            .expect("fetch resolver account")
            .expect("resolver account must exist");
        accounts.insert(
            key,
            SplAccount {
                key,
                owner: account.owner,
                data: account.data,
                executable: account.executable,
            },
        );
    }
    accounts
}

async fn resolve(
    role: LegRole,
    leg: SplTransferLeg,
    fetched: &HashMap<Pubkey, SplAccount>,
) -> LegHook {
    let options = ResolveOptions::default().with_expected_hook_program(HOOK_PROGRAM_ID);
    resolve_leg(role, leg, &options, |key| {
        let account = fetched.get(&key).cloned();
        async move { Ok::<_, FetchError>(account) }
    })
    .await
    .expect("resolve SPL Transfer Hook accounts")
}

async fn framed_swap(
    context: &mut ProgramTestContext,
    fixture: &Fixture,
    amount_in: u64,
    expected_amount_out: u64,
) -> Instruction {
    let payer = context.payer.pubkey();
    let fetched = fetch_resolver_accounts(context, fixture).await;
    let input_leg = resolve(
        LegRole::Input,
        SplTransferLeg {
            source: fixture.trader_in.pubkey(),
            mint: fixture.mint_0.pubkey(),
            destination: fixture.vault_0,
            authority: payer,
            amount: amount_in,
        },
        &fetched,
    )
    .await;
    // In CLMM the pool-state PDA owns the vaults and signs the output transfer.
    let output_leg = resolve(
        LegRole::Output,
        SplTransferLeg {
            source: fixture.vault_1,
            mint: fixture.mint_1.pubkey(),
            destination: fixture.trader_out.pubkey(),
            authority: fixture.pool_state,
            amount: expected_amount_out,
        },
        &fetched,
    )
    .await;
    assert_ne!(
        input_leg.slice(),
        output_leg.slice(),
        "the two legs must carry independent hook slices"
    );
    let accounts = ClmmSwapAccounts {
        payer,
        amm_config: fixture.amm_config,
        pool_state: fixture.pool_state,
        input_token_account: fixture.trader_in.pubkey(),
        output_token_account: fixture.trader_out.pubkey(),
        input_vault: fixture.vault_0,
        output_vault: fixture.vault_1,
        observation_state: fixture.observation,
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        memo_program: MEMO_PROGRAM_ID,
        input_vault_mint: fixture.mint_0.pubkey(),
        output_vault_mint: fixture.mint_1.pubkey(),
    };
    let mut instruction = build_clmm_swap_v2(
        CLMM_PROGRAM_ID,
        &accounts,
        &fixture.tick_arrays,
        None,
        ClmmSwapArgs {
            amount: amount_in,
            other_amount_threshold: 1,
            sqrt_price_limit_x64: 0,
            is_base_input: true,
        },
    );
    frame_clmm_swap_v3(&mut instruction, 2, 0, &input_leg, &output_leg)
        .expect("frame tick arrays and the two independent hook slices");
    instruction
}

#[tokio::test]
#[ignore = "requires raydium_clmm.so and reference_hook_onchain.so in SBF_OUT_DIR"]
async fn clmm_sbf_v3_executes_both_token_2022_hook_legs_and_rolls_back_rejections() {
    let mut mint_0 = Keypair::new();
    let mut mint_1 = Keypair::new();
    if mint_0.pubkey() > mint_1.pubkey() {
        std::mem::swap(&mut mint_0, &mut mint_1);
    }
    let (amm_config, _) =
        Pubkey::find_program_address(&[AMM_CONFIG_SEED, &0u16.to_be_bytes()], &CLMM_PROGRAM_ID);
    let (pool_state, _) = Pubkey::find_program_address(
        &[
            POOL_SEED,
            amm_config.as_ref(),
            mint_0.pubkey().as_ref(),
            mint_1.pubkey().as_ref(),
        ],
        &CLMM_PROGRAM_ID,
    );
    let vault = |mint: &Keypair| {
        Pubkey::find_program_address(
            &[POOL_VAULT_SEED, pool_state.as_ref(), mint.pubkey().as_ref()],
            &CLMM_PROGRAM_ID,
        )
        .0
    };
    let fixture = Fixture {
        amm_config,
        pool_state,
        vault_0: vault(&mint_0),
        vault_1: vault(&mint_1),
        mint_0,
        mint_1,
        observation: Pubkey::find_program_address(
            &[OBSERVATION_SEED, pool_state.as_ref()],
            &CLMM_PROGRAM_ID,
        )
        .0,
        bitmap: Pubkey::find_program_address(&[BITMAP_SEED, pool_state.as_ref()], &CLMM_PROGRAM_ID)
            .0,
        trader_in: Keypair::new(),
        trader_out: Keypair::new(),
        tick_arrays: [
            tick_array_address(&pool_state, UPPER_ARRAY_START),
            tick_array_address(&pool_state, LOWER_ARRAY_START),
        ],
    };
    let provider_0 = Keypair::new();
    let provider_1 = Keypair::new();
    let position_nft_mint = Keypair::new();

    let mut program_test = ProgramTest::new(
        "reference_hook_onchain",
        HOOK_PROGRAM_ID,
        processor!(process_instruction),
    );
    program_test.add_program("raydium_clmm", CLMM_PROGRAM_ID, None);
    let rent = solana_sdk::rent::Rent::default();
    program_test.add_account(
        fixture.amm_config,
        amm_config_account(rent.minimum_balance(117)),
    );
    for mint in [&fixture.mint_0, &fixture.mint_1] {
        let key = Pubkey::find_program_address(
            &[SUPPORT_MINT_SEED, mint.pubkey().as_ref()],
            &CLMM_PROGRAM_ID,
        )
        .0;
        program_test.add_account(
            key,
            support_mint_account(mint.pubkey(), rent.minimum_balance(105)),
        );
    }
    let mut context = program_test.start_with_context().await;
    let payer = context.payer.pubkey();

    // create_pool requires block_timestamp > open_time (0).
    let mut clock: solana_sdk::clock::Clock = context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = 1_700_000_000;
    context.set_sysvar(&clock);

    // 1. Mints (TransferHook extension present, hook program still unset) and funded accounts.
    let mint_len = ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook])
        .expect("calculate Token-2022 mint size");
    let ctx_rent = context.banks_client.get_rent().await.unwrap();
    let mut setup = Vec::new();
    for mint in [&fixture.mint_0, &fixture.mint_1] {
        setup.push(system_instruction::create_account(
            &payer,
            &mint.pubkey(),
            ctx_rent.minimum_balance(mint_len),
            mint_len as u64,
            &spl_token_2022::id(),
        ));
        setup.push(
            transfer_hook_instruction::initialize(
                &spl_token_2022::id(),
                &mint.pubkey(),
                Some(payer),
                None,
            )
            .unwrap(),
        );
        setup.push(
            token_2022_instruction::initialize_mint2(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &payer,
                None,
                6,
            )
            .unwrap(),
        );
    }
    for (account, mint) in [
        (&provider_0, &fixture.mint_0),
        (&provider_1, &fixture.mint_1),
        (&fixture.trader_in, &fixture.mint_0),
        (&fixture.trader_out, &fixture.mint_1),
    ] {
        setup.extend(token_account_create_instructions(
            payer,
            account.pubkey(),
            mint.pubkey(),
            payer,
            &ctx_rent,
        ));
    }
    for (mint, account, amount) in [
        (&fixture.mint_0, &provider_0, 2_000_000_000u64),
        (&fixture.mint_1, &provider_1, 2_000_000_000u64),
        (&fixture.mint_0, &fixture.trader_in, 1_000u64),
    ] {
        setup.push(
            token_2022_instruction::mint_to(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &account.pubkey(),
                &payer,
                &[],
                amount,
            )
            .unwrap(),
        );
    }
    send(
        &mut context,
        &setup,
        &[
            &fixture.mint_0,
            &fixture.mint_1,
            &provider_0,
            &provider_1,
            &fixture.trader_in,
            &fixture.trader_out,
        ],
    )
    .await
    .expect("create Token-2022 mints and funded accounts");

    // 2. create_pool at price 1 (sqrt_price_x64 = 2^64), real instruction.
    let mut data = discriminator("global:create_pool").to_vec();
    data.extend_from_slice(&(1u128 << 64).to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    let support = |mint: &Keypair| {
        Pubkey::find_program_address(
            &[SUPPORT_MINT_SEED, mint.pubkey().as_ref()],
            &CLMM_PROGRAM_ID,
        )
        .0
    };
    let create_pool = Instruction {
        program_id: CLMM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(fixture.amm_config, false),
            AccountMeta::new(fixture.pool_state, false),
            AccountMeta::new_readonly(fixture.mint_0.pubkey(), false),
            AccountMeta::new_readonly(fixture.mint_1.pubkey(), false),
            AccountMeta::new(fixture.vault_0, false),
            AccountMeta::new(fixture.vault_1, false),
            AccountMeta::new(fixture.observation, false),
            AccountMeta::new(fixture.bitmap, false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(support(&fixture.mint_0), false),
            AccountMeta::new_readonly(support(&fixture.mint_1), false),
        ],
        data,
    };
    send(&mut context, &with_budget(create_pool), &[])
        .await
        .expect("create a real CLMM pool over two Token-2022 mints");
    let pool_account = context
        .banks_client
        .get_account(fixture.pool_state)
        .await
        .unwrap()
        .expect("CLMM pool exists");
    assert_eq!(pool_account.owner, CLMM_PROGRAM_ID);

    // 3. Real liquidity position (NFT-backed) over ticks [-300, 300]: this creates both tick
    //    arrays and moves both tokens into the vaults while the hook is still unset.
    let nft_account = Pubkey::find_program_address(
        &[
            payer.as_ref(),
            spl_token_2022::id().as_ref(),
            position_nft_mint.pubkey().as_ref(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0;
    let personal_position = Pubkey::find_program_address(
        &[POSITION_SEED, position_nft_mint.pubkey().as_ref()],
        &CLMM_PROGRAM_ID,
    )
    .0;
    let mut data = discriminator("global:open_position_with_token22_nft").to_vec();
    data.extend_from_slice(&TICK_LOWER.to_le_bytes());
    data.extend_from_slice(&TICK_UPPER.to_le_bytes());
    data.extend_from_slice(&LOWER_ARRAY_START.to_le_bytes());
    data.extend_from_slice(&UPPER_ARRAY_START.to_le_bytes());
    data.extend_from_slice(&LIQUIDITY.to_le_bytes());
    data.extend_from_slice(&DEPOSIT_MAX.to_le_bytes());
    data.extend_from_slice(&DEPOSIT_MAX.to_le_bytes());
    data.push(0); // with_metadata = false
    data.push(0); // base_flag = None
    let open_position = Instruction {
        program_id: CLMM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(payer, false), // position NFT owner
            AccountMeta::new(position_nft_mint.pubkey(), true),
            AccountMeta::new(nft_account, false),
            AccountMeta::new(fixture.pool_state, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // deprecated protocol_position
            AccountMeta::new(
                tick_array_address(&fixture.pool_state, LOWER_ARRAY_START),
                false,
            ),
            AccountMeta::new(
                tick_array_address(&fixture.pool_state, UPPER_ARRAY_START),
                false,
            ),
            AccountMeta::new(personal_position, false),
            AccountMeta::new(provider_0.pubkey(), false),
            AccountMeta::new(provider_1.pubkey(), false),
            AccountMeta::new(fixture.vault_0, false),
            AccountMeta::new(fixture.vault_1, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(spl_token_2022::id(), false),
            AccountMeta::new_readonly(fixture.mint_0.pubkey(), false),
            AccountMeta::new_readonly(fixture.mint_1.pubkey(), false),
        ],
        data,
    };
    send(
        &mut context,
        &with_budget(open_position),
        &[&position_nft_mint],
    )
    .await
    .expect("open a real CLMM liquidity position with Token-2022 liquidity");
    let vault_0_seeded = token_amount(&mut context, fixture.vault_0).await;
    let vault_1_seeded = token_amount(&mut context, fixture.vault_1).await;
    assert!(
        vault_0_seeded > 0 && vault_1_seeded > 0,
        "position must fund both vaults"
    );

    // 4. Enable the hook on both mints now that liquidity is in place (the helper-based
    //    liquidity paths reject hooked mints; only swap_v3 carries hook slices).
    let mut enable = Vec::new();
    for (mint, limit) in [
        (&fixture.mint_0, INPUT_HOOK_LIMIT),
        (&fixture.mint_1, OUTPUT_HOOK_LIMIT),
    ] {
        enable.push(
            transfer_hook_instruction::update(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &payer,
                &[],
                Some(HOOK_PROGRAM_ID),
            )
            .unwrap(),
        );
        enable.push(initialize_hook_instruction(
            HOOK_PROGRAM_ID,
            mint.pubkey(),
            payer,
            payer,
            &InitializeHookArgs::max_transfer(
                AuthorityMode::ExtensionAuthority,
                limit,
                Pubkey::default(),
            ),
        ));
    }
    send(&mut context, &enable, &[])
        .await
        .expect("enable the hook on both mints and initialize their configs");

    // 5. Successful hooked swap: both hook legs must run.
    let trader_in_before = token_amount(&mut context, fixture.trader_in.pubkey()).await;
    let trader_out_before = token_amount(&mut context, fixture.trader_out.pubkey()).await;
    assert_eq!(trader_in_before, 1_000);
    assert_eq!(trader_out_before, 0);

    let swap = framed_swap(&mut context, &fixture, 10, 8).await;
    let swap_tx = Transaction::new_signed_with_payer(
        &with_budget(swap),
        Some(&payer),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    let trace = trace_hook(&mut context, swap_tx.clone()).await;
    assert!(trace.succeeded, "simulated CLMM swap_v3 must succeed");
    assert_eq!(
        trace.hook_invocations, 2,
        "hook Execute must run once per hooked transfer leg (input then output)"
    );
    assert!(!trace.hook_rejected_over_limit);
    context
        .banks_client
        .process_transaction(swap_tx)
        .await
        .expect("CLMM swap_v3 must run both independent Transfer Hook CPIs");

    let trader_in_after = token_amount(&mut context, fixture.trader_in.pubkey()).await;
    let trader_out_after = token_amount(&mut context, fixture.trader_out.pubkey()).await;
    let vault_0_after = token_amount(&mut context, fixture.vault_0).await;
    let vault_1_after = token_amount(&mut context, fixture.vault_1).await;
    assert_eq!(trader_in_after, 990, "10 units in");
    assert!(trader_out_after > 0, "swap must pay out token 1");
    println!(
        "clmm swap_v3: 10 in -> {trader_out_after} out; vaults {vault_0_seeded}->{vault_0_after} / {vault_1_seeded}->{vault_1_after}"
    );
    assert_eq!(vault_0_after, vault_0_seeded + 10);
    assert_eq!(vault_1_after, vault_1_seeded - trader_out_after);
    let pool_after_success = raw_account_data(&mut context, fixture.pool_state).await;

    // 6. Output-leg rejection: 100 in passes the input limit (500) but the output exceeds 20.
    let rejected = framed_swap(&mut context, &fixture, 100, 98).await;
    let rejected_tx = Transaction::new_signed_with_payer(
        &with_budget(rejected),
        Some(&payer),
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
        trace.hook_rejected_over_limit,
        "rejection must originate in the hook program"
    );
    assert!(context
        .banks_client
        .process_transaction(rejected_tx)
        .await
        .is_err());
    assert_eq!(
        token_amount(&mut context, fixture.trader_in.pubkey()).await,
        trader_in_after
    );
    assert_eq!(
        token_amount(&mut context, fixture.trader_out.pubkey()).await,
        trader_out_after
    );
    assert_eq!(
        token_amount(&mut context, fixture.vault_0).await,
        vault_0_after
    );
    assert_eq!(
        token_amount(&mut context, fixture.vault_1).await,
        vault_1_after
    );
    assert_eq!(
        raw_account_data(&mut context, fixture.pool_state).await,
        pool_after_success,
        "a rejected swap must leave the whole pool state untouched"
    );

    // 7. Input-leg rejection: 600 in exceeds the input limit before the output leg runs.
    let input_rejected = framed_swap(&mut context, &fixture, 600, 597).await;
    let input_rejected_tx = Transaction::new_signed_with_payer(
        &with_budget(input_rejected),
        Some(&payer),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    let trace = trace_hook(&mut context, input_rejected_tx.clone()).await;
    assert!(!trace.succeeded);
    assert_eq!(
        trace.hook_invocations, 1,
        "input-leg rejection aborts before the output leg hook runs"
    );
    assert!(trace.hook_rejected_over_limit);
    assert!(context
        .banks_client
        .process_transaction(input_rejected_tx)
        .await
        .is_err());
    assert_eq!(
        token_amount(&mut context, fixture.trader_in.pubkey()).await,
        trader_in_after
    );
    assert_eq!(
        token_amount(&mut context, fixture.trader_out.pubkey()).await,
        trader_out_after
    );
    assert_eq!(
        token_amount(&mut context, fixture.vault_0).await,
        vault_0_after
    );
    assert_eq!(
        token_amount(&mut context, fixture.vault_1).await,
        vault_1_after
    );
    assert_eq!(
        raw_account_data(&mut context, fixture.pool_state).await,
        pool_after_success
    );
}
