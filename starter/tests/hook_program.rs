//! Hook-program behavior tests. Every failure asserts the exact error code.
//!
//! Mints and token accounts are pre-loaded as Token-2022 owned account data so that guard paths
//! (for example a forged `transferring` flag, which only Token-2022 can set on-chain) can be
//! exercised directly. Transfers that go through the real Token-2022 program use the same
//! pre-loaded accounts. The hook runs natively unless `SBF_OUT_DIR` points at a directory
//! holding `transfer_hook_starter.so`, in which case ProgramTest runs the SBF build.

use {
    solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext},
    solana_sdk::{
        account::{Account, AccountSharedData},
        instruction::{AccountMeta, Instruction, InstructionError},
        program_option::COption,
        program_pack::Pack,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        transaction::{Transaction, TransactionError},
    },
    spl_tlv_account_resolution::state::ExtraAccountMetaList,
    spl_token_2022::{
        extension::{
            transfer_hook::{TransferHook, TransferHookAccount},
            BaseStateWithExtensionsMut, ExtensionType, StateWithExtensions, StateWithExtensionsMut,
        },
        instruction as token_instruction,
        state::{Account as TokenAccount, AccountState, Mint},
    },
    spl_transfer_hook_interface::instruction::ExecuteInstruction,
    transfer_hook_starter::{
        config_address, config_extra_account_meta, initialize_hook_instruction,
        process_instruction, validation_list_address, HookConfig, HookError, InitializeHookArgs,
        CONFIG_HEADER_LEN, MAX_PARAMS_LEN, VALIDATION_LIST_LEN,
    },
};

const HOOK: Pubkey = Pubkey::new_from_array([47; 32]);
const OTHER_PROGRAM: Pubkey = Pubkey::new_from_array([48; 32]);
const FUNDS: u64 = 1_000_000_000;

// ------------------------------------------------------------------------------------------
// Account builders
// ------------------------------------------------------------------------------------------

fn coption(key: Option<Pubkey>) -> COption<Pubkey> {
    match key {
        Some(key) => COption::Some(key),
        None => COption::None,
    }
}

struct MintSpec {
    owner: Pubkey,
    hook_program: Option<Pubkey>,
    ext_authority: Option<Pubkey>,
    mint_authority: Option<Pubkey>,
    with_extension: bool,
    supply: u64,
}

impl MintSpec {
    fn hooked(ext_authority: Option<Pubkey>, mint_authority: Option<Pubkey>) -> Self {
        MintSpec {
            owner: spl_token_2022::id(),
            hook_program: Some(HOOK),
            ext_authority,
            mint_authority,
            with_extension: true,
            supply: u64::MAX,
        }
    }

    fn account(&self) -> Account {
        let base = Mint {
            mint_authority: coption(self.mint_authority),
            supply: self.supply,
            decimals: 0,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let data = if self.with_extension {
            let len =
                ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook])
                    .unwrap();
            let mut data = vec![0u8; len];
            let mut state =
                StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut data).unwrap();
            state.base = base;
            state.pack_base();
            state.init_account_type().unwrap();
            state.init_extension::<TransferHook>(true).unwrap();
            // TransferHook { authority, program_id }: two optional non-zero pubkeys.
            let bytes = state.get_extension_bytes_mut::<TransferHook>().unwrap();
            bytes[..32].copy_from_slice(self.ext_authority.unwrap_or_default().as_ref());
            bytes[32..].copy_from_slice(self.hook_program.unwrap_or_default().as_ref());
            data
        } else {
            let mut data = vec![0u8; Mint::LEN];
            Mint::pack(base, &mut data).unwrap();
            data
        };
        Account {
            lamports: FUNDS,
            data,
            owner: self.owner,
            executable: false,
            rent_epoch: 0,
        }
    }
}

fn token_account(mint: Pubkey, owner: Pubkey, amount: u64, transferring: bool) -> Account {
    let len = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
        ExtensionType::TransferHookAccount,
    ])
    .unwrap();
    let mut data = vec![0u8; len];
    let mut state =
        StateWithExtensionsMut::<TokenAccount>::unpack_uninitialized(&mut data).unwrap();
    state.base = TokenAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    state.pack_base();
    state.init_account_type().unwrap();
    state
        .init_extension::<TransferHookAccount>(true)
        .unwrap()
        .transferring = transferring.into();
    Account {
        lamports: FUNDS,
        data,
        owner: spl_token_2022::id(),
        executable: false,
        rent_epoch: 0,
    }
}

fn new_test() -> ProgramTest {
    ProgramTest::new(
        "transfer_hook_starter",
        HOOK,
        processor!(process_instruction),
    )
}

fn add_mint(test: &mut ProgramTest, spec: &MintSpec) -> Pubkey {
    let key = Pubkey::new_unique();
    test.add_account(key, spec.account());
    key
}

fn add_token(
    test: &mut ProgramTest,
    mint: Pubkey,
    owner: &Pubkey,
    amount: u64,
    transferring: bool,
) -> Pubkey {
    let key = Pubkey::new_unique();
    test.add_account(key, token_account(mint, *owner, amount, transferring));
    key
}

// ------------------------------------------------------------------------------------------
// Transaction helpers
// ------------------------------------------------------------------------------------------

async fn send(
    ctx: &mut ProgramTestContext,
    instructions: &[Instruction],
    extra_signers: &[&Keypair],
) -> Result<(), BanksClientError> {
    let blockhash = ctx.get_new_latest_blockhash().await.unwrap();
    let mut signers: Vec<&Keypair> = vec![&ctx.payer];
    signers.extend_from_slice(extra_signers);
    let tx = Transaction::new_signed_with_payer(
        instructions,
        Some(&ctx.payer.pubkey()),
        &signers,
        blockhash,
    );
    ctx.banks_client.process_transaction(tx).await
}

fn instruction_error(result: Result<(), BanksClientError>) -> InstructionError {
    match result {
        Err(BanksClientError::TransactionError(TransactionError::InstructionError(_, error))) => {
            error
        }
        other => panic!("expected an instruction error, got {other:?}"),
    }
}

#[track_caller]
fn expect_hook_error(result: Result<(), BanksClientError>, expected: HookError) {
    match instruction_error(result) {
        InstructionError::Custom(code) => {
            assert_eq!(
                code,
                expected.code(),
                "expected {expected}, got {:?}",
                HookError::from_code(code)
            )
        }
        other => panic!("expected {expected}, got {other:?}"),
    }
}

#[track_caller]
fn expect_instruction_error(result: Result<(), BanksClientError>, expected: InstructionError) {
    assert_eq!(instruction_error(result), expected);
}

async fn fetch(ctx: &mut ProgramTestContext, key: Pubkey) -> Option<Account> {
    ctx.banks_client.get_account(key).await.unwrap()
}

async fn fetch_config_data(ctx: &mut ProgramTestContext, mint: Pubkey) -> Vec<u8> {
    let account = fetch(ctx, config_address(&mint, &HOOK).0)
        .await
        .expect("config account exists");
    assert_eq!(account.owner, HOOK);
    HookConfig::decode(&account.data).expect("config decodes strictly");
    account.data
}

async fn fetched_limit(ctx: &mut ProgramTestContext, mint: Pubkey) -> u64 {
    let data = fetch_config_data(ctx, mint).await;
    HookConfig::decode(&data)
        .unwrap()
        .max_transfer_limit()
        .unwrap()
}

async fn init_hook(
    ctx: &mut ProgramTestContext,
    mint: Pubkey,
    authority: &Keypair,
    args: &InitializeHookArgs,
) -> Result<(), BanksClientError> {
    let instruction =
        initialize_hook_instruction(HOOK, mint, authority.pubkey(), ctx.payer.pubkey(), args);
    send(ctx, &[instruction], &[authority]).await
}

fn max_args(limit: u64) -> InitializeHookArgs {
    InitializeHookArgs::max_transfer(limit)
}

fn hooked_transfer(
    source: Pubkey,
    mint: Pubkey,
    destination: Pubkey,
    owner: &Pubkey,
    amount: u64,
) -> Instruction {
    let mut instruction = token_instruction::transfer_checked(
        &spl_token_2022::id(),
        &source,
        &mint,
        &destination,
        owner,
        &[],
        amount,
        0,
    )
    .unwrap();
    instruction.accounts.extend([
        AccountMeta::new_readonly(config_address(&mint, &HOOK).0, false),
        AccountMeta::new_readonly(HOOK, false),
        AccountMeta::new_readonly(validation_list_address(&mint, &HOOK).0, false),
    ]);
    instruction
}

fn direct_execute(
    source: Pubkey,
    mint: Pubkey,
    destination: Pubkey,
    config: Pubkey,
    list: Pubkey,
    payer: Pubkey,
    amount: u64,
) -> Instruction {
    spl_transfer_hook_interface::instruction::execute_with_extra_account_metas(
        &HOOK,
        &source,
        &mint,
        &destination,
        &payer,
        &list,
        &[AccountMeta::new_readonly(config, false)],
        amount,
    )
}

async fn amount_of(ctx: &mut ProgramTestContext, account: Pubkey) -> u64 {
    let account = fetch(ctx, account).await.unwrap();
    StateWithExtensions::<TokenAccount>::unpack(&account.data)
        .unwrap()
        .base
        .amount
}

// ------------------------------------------------------------------------------------------
// InitializeHook: mint validation (B1)
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn initialize_rejects_mints_that_are_not_valid_token_2022_hook_mints() {
    let authority = Keypair::new();
    let mut test = new_test();
    let mut fake_owner = MintSpec::hooked(Some(authority.pubkey()), Some(authority.pubkey()));
    fake_owner.owner = OTHER_PROGRAM;
    let fake_owner = add_mint(&mut test, &fake_owner);
    let mut system_owned = MintSpec::hooked(Some(authority.pubkey()), Some(authority.pubkey()));
    system_owned.owner = solana_sdk::system_program::id();
    let system_owned = add_mint(&mut test, &system_owned);
    let mut no_extension = MintSpec::hooked(Some(authority.pubkey()), Some(authority.pubkey()));
    no_extension.with_extension = false;
    let no_extension = add_mint(&mut test, &no_extension);
    let mut other_program = MintSpec::hooked(Some(authority.pubkey()), Some(authority.pubkey()));
    other_program.hook_program = Some(OTHER_PROGRAM);
    let other_program = add_mint(&mut test, &other_program);
    let mut no_program = MintSpec::hooked(Some(authority.pubkey()), Some(authority.pubkey()));
    no_program.hook_program = None;
    let no_program = add_mint(&mut test, &no_program);
    let mut ctx = test.start_with_context().await;
    let args = max_args(50);

    for (mint, expected) in [
        (fake_owner, HookError::MintOwnerNotToken2022),
        (system_owned, HookError::MintOwnerNotToken2022),
        (no_extension, HookError::MintHookExtensionMissing),
        (other_program, HookError::MintHookProgramMismatch),
        (no_program, HookError::MintHookProgramMismatch),
    ] {
        expect_hook_error(init_hook(&mut ctx, mint, &authority, &args).await, expected);
        // Nothing was created by the failed attempt.
        assert!(fetch(&mut ctx, config_address(&mint, &HOOK).0)
            .await
            .is_none());
        assert!(fetch(&mut ctx, validation_list_address(&mint, &HOOK).0)
            .await
            .is_none());
    }
}

// ------------------------------------------------------------------------------------------
// InitializeHook: who may set the hook up
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn only_the_live_hook_authority_can_initialize() {
    let ext = Keypair::new();
    let stranger = Keypair::new();
    let mut test = new_test();
    let good = add_mint(&mut test, &MintSpec::hooked(Some(ext.pubkey()), None));
    let revoked = add_mint(&mut test, &MintSpec::hooked(None, Some(ext.pubkey())));
    let mut ctx = test.start_with_context().await;
    let args = max_args(50);

    // A stranger, and the mint authority (which is a different power), are refused.
    expect_hook_error(
        init_hook(&mut ctx, good, &stranger, &args).await,
        HookError::AuthorityMismatch,
    );
    let mint_authority = Keypair::new();
    expect_hook_error(
        init_hook(&mut ctx, good, &mint_authority, &args).await,
        HookError::AuthorityMismatch,
    );
    // A revoked extension authority means nobody can initialise.
    expect_hook_error(
        init_hook(&mut ctx, revoked, &ext, &args).await,
        HookError::AuthorityUnavailable,
    );
    // The authority must actually sign.
    let mut unsigned =
        initialize_hook_instruction(HOOK, good, ext.pubkey(), ctx.payer.pubkey(), &args);
    unsigned.accounts[3].is_signer = false;
    expect_instruction_error(
        send(&mut ctx, &[unsigned], &[]).await,
        InstructionError::MissingRequiredSignature,
    );
    // Nothing was created by the failed attempts.
    assert!(fetch(&mut ctx, config_address(&good, &HOOK).0)
        .await
        .is_none());

    // The live authority succeeds, and the stored state is exactly what was asked for.
    init_hook(&mut ctx, good, &ext, &max_args(1234))
        .await
        .unwrap();
    let config_data = fetch_config_data(&mut ctx, good).await;
    let config = HookConfig::decode(&config_data).unwrap();
    assert_eq!(config.mint, good);
    assert_eq!(config.max_transfer_limit().unwrap(), 1234);
    let (address, bump) = config_address(&good, &HOOK);
    assert_eq!(config.bump, bump);
    config.verify_address(&HOOK, &good, &address).unwrap();
    assert_eq!(config.list_bump, validation_list_address(&good, &HOOK).1);
}

#[tokio::test]
async fn initialize_rejects_invalid_params_and_accounts() {
    let ext = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mut ctx = test.start_with_context().await;
    let base = max_args(50);

    let mut args = base.clone();
    args.params = 0u64.to_le_bytes().to_vec();
    expect_hook_error(
        init_hook(&mut ctx, mint, &ext, &args).await,
        HookError::InvalidParams,
    );
    let mut args = base.clone();
    args.params = vec![1; 7];
    expect_hook_error(
        init_hook(&mut ctx, mint, &ext, &args).await,
        HookError::InvalidParams,
    );
    let mut args = base.clone();
    args.params = vec![1; MAX_PARAMS_LEN + 1];
    expect_hook_error(
        init_hook(&mut ctx, mint, &ext, &args).await,
        HookError::ParamsTooLarge,
    );

    // Substituted PDAs and a wrong system program.
    let build = |ctx: &ProgramTestContext| {
        initialize_hook_instruction(HOOK, mint, ext.pubkey(), ctx.payer.pubkey(), &base)
    };
    let mut instruction = build(&ctx);
    instruction.accounts[0].pubkey = config_address(&Pubkey::new_unique(), &HOOK).0;
    expect_hook_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        HookError::InvalidConfigPda,
    );
    let mut instruction = build(&ctx);
    instruction.accounts[1].pubkey = validation_list_address(&Pubkey::new_unique(), &HOOK).0;
    expect_hook_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        HookError::InvalidValidationList,
    );
    let mut instruction = build(&ctx);
    instruction.accounts[5].pubkey = OTHER_PROGRAM;
    expect_instruction_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        InstructionError::IncorrectProgramId,
    );
    let mut instruction = build(&ctx);
    instruction.accounts[0].is_writable = false;
    expect_instruction_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        InstructionError::Immutable,
    );
    let mut instruction = build(&ctx);
    instruction.accounts.pop();
    expect_instruction_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        InstructionError::NotEnoughAccountKeys,
    );
    // Truncated instruction data.
    let mut instruction = build(&ctx);
    instruction.data.truncate(12);
    expect_instruction_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        InstructionError::InvalidInstructionData,
    );
}

// ------------------------------------------------------------------------------------------
// InitializeHook: re-init, griefing, atomicity (B2)
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn second_initialize_fails_with_already_initialized_and_changes_nothing() {
    let ext = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mut ctx = test.start_with_context().await;
    init_hook(&mut ctx, mint, &ext, &max_args(50))
        .await
        .unwrap();
    let before = fetch(&mut ctx, config_address(&mint, &HOOK).0)
        .await
        .unwrap();
    for args in [max_args(50), max_args(9_999)] {
        expect_hook_error(
            init_hook(&mut ctx, mint, &ext, &args).await,
            HookError::AlreadyInitialized,
        );
    }
    let after = fetch(&mut ctx, config_address(&mint, &HOOK).0)
        .await
        .unwrap();
    assert_eq!(before.data, after.data);
}

#[tokio::test]
async fn initialize_survives_prefunded_pdas() {
    let ext = Keypair::new();
    let mut test = new_test();
    let mints: Vec<Pubkey> = (0..3)
        .map(|_| {
            add_mint(
                &mut test,
                &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
            )
        })
        .collect();
    let mut ctx = test.start_with_context().await;
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let config_rent = rent.minimum_balance(CONFIG_HEADER_LEN + 8);
    let list_rent = rent.minimum_balance(VALIDATION_LIST_LEN);

    // (config pre-fund, list pre-fund). The runtime refuses to create a rent-paying account, so
    // the cheapest grief is the zero-data rent-exempt minimum; also exact rent and over-funded.
    let min_empty = rent.minimum_balance(0);
    let prefunds = [
        (min_empty, min_empty),
        (config_rent, list_rent),
        (config_rent + 5_000_000, min_empty),
    ];
    for (mint, (config_funds, list_funds)) in mints.into_iter().zip(prefunds) {
        let config_key = config_address(&mint, &HOOK).0;
        let list_key = validation_list_address(&mint, &HOOK).0;
        let payer = ctx.payer.pubkey();
        send(
            &mut ctx,
            &[
                system_instruction::transfer(&payer, &config_key, config_funds),
                system_instruction::transfer(&payer, &list_key, list_funds),
            ],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(
            fetch(&mut ctx, config_key).await.unwrap().lamports,
            config_funds
        );

        init_hook(&mut ctx, mint, &ext, &max_args(50))
            .await
            .expect("pre-funded PDAs must not block initialization");

        let config = fetch(&mut ctx, config_key).await.unwrap();
        let list = fetch(&mut ctx, list_key).await.unwrap();
        assert_eq!(config.owner, HOOK);
        assert_eq!(list.owner, HOOK);
        assert_eq!(config.data.len(), CONFIG_HEADER_LEN + 8);
        assert_eq!(list.data.len(), VALIDATION_LIST_LEN);
        assert!(config.lamports >= config_rent.max(config_funds));
        assert!(list.lamports >= list_rent.max(list_funds));
        HookConfig::decode(&config.data).unwrap();
    }
}

#[tokio::test]
async fn validation_list_is_identical_for_every_mint_and_is_seeds_based() {
    let ext = Keypair::new();
    let mut test = new_test();
    let mints: Vec<Pubkey> = (0..2)
        .map(|_| {
            add_mint(
                &mut test,
                &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
            )
        })
        .collect();
    let mut ctx = test.start_with_context().await;
    let mut lists = Vec::new();
    for (index, mint) in mints.iter().enumerate() {
        init_hook(&mut ctx, *mint, &ext, &max_args(10 + index as u64))
            .await
            .unwrap();
        lists.push(
            fetch(&mut ctx, validation_list_address(mint, &HOOK).0)
                .await
                .unwrap()
                .data,
        );
    }
    assert_eq!(lists[0], lists[1]);
    let mut canonical = vec![0u8; VALIDATION_LIST_LEN];
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut canonical,
        &[config_extra_account_meta().unwrap()],
    )
    .unwrap();
    assert_eq!(lists[0], canonical);
}

// ------------------------------------------------------------------------------------------
// Execute: direct invocation, flags, substitution, corrupt state, account count (F4)
// ------------------------------------------------------------------------------------------

struct CorruptCase {
    mint: Pubkey,
    source: Pubkey,
    destination: Pubkey,
    expected: HookError,
}

const GOOD_CONFIG_PARAMS: [u8; 8] = 50u64.to_le_bytes();

fn good_config(mint: Pubkey) -> HookConfig<'static> {
    HookConfig::new(
        config_address(&mint, &HOOK).1,
        validation_list_address(&mint, &HOOK).1,
        mint,
        &GOOD_CONFIG_PARAMS,
    )
    .unwrap()
}

fn program_account(owner: Pubkey, data: Vec<u8>) -> Account {
    Account {
        lamports: FUNDS,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn canonical_list() -> Vec<u8> {
    let mut data = vec![0u8; VALIDATION_LIST_LEN];
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut data,
        &[config_extra_account_meta().unwrap()],
    )
    .unwrap();
    data
}

#[tokio::test]
async fn direct_execute_requires_the_transferring_flag_on_both_accounts() {
    let ext = Keypair::new();
    let mut test = new_test();
    let owner = Pubkey::new_unique();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let other_mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let flagged_a = add_token(&mut test, mint, &owner, 100, true);
    let flagged_b = add_token(&mut test, mint, &owner, 0, true);
    let plain_a = add_token(&mut test, mint, &owner, 100, false);
    let plain_b = add_token(&mut test, mint, &owner, 0, false);
    let foreign_flagged = add_token(&mut test, other_mint, &owner, 0, true);
    let mut ctx = test.start_with_context().await;
    init_hook(&mut ctx, mint, &ext, &max_args(50))
        .await
        .unwrap();
    let config = config_address(&mint, &HOOK).0;
    let list = validation_list_address(&mint, &HOOK).0;
    let payer = ctx.payer.pubkey();
    let run = |source, destination, amount| {
        direct_execute(source, mint, destination, config, list, payer, amount)
    };

    for (source, destination) in [
        (flagged_a, plain_b), // flag on source only
        (plain_a, flagged_b), // flag on destination only
        (plain_a, plain_b),   // neither
    ] {
        expect_hook_error(
            send(&mut ctx, &[run(source, destination, 1)], &[]).await,
            HookError::NotDirectInvocation,
        );
    }
    // A flagged account of a different mint is not accepted for this mint.
    expect_hook_error(
        send(&mut ctx, &[run(flagged_a, foreign_flagged, 1)], &[]).await,
        HookError::AccountOrderMismatch,
    );
    // Both flags set (state only Token-2022 can produce on-chain): the rule itself applies.
    send(&mut ctx, &[run(flagged_a, flagged_b, 50)], &[])
        .await
        .expect("amount == limit passes");
    expect_hook_error(
        send(&mut ctx, &[run(flagged_a, flagged_b, 51)], &[]).await,
        HookError::TransferExceedsLimit,
    );

    // A token account owned by another program is rejected before anything else.
    let forged = Pubkey::new_unique();
    let template = fetch(&mut ctx, flagged_a).await.unwrap();
    ctx.set_account(
        &forged,
        &AccountSharedData::from(Account {
            owner: OTHER_PROGRAM,
            ..template
        }),
    );
    expect_instruction_error(
        send(&mut ctx, &[run(forged, flagged_b, 1)], &[]).await,
        InstructionError::IncorrectProgramId,
    );
}

#[tokio::test]
async fn execute_rejects_substituted_config_and_list_from_another_mint() {
    let ext = Keypair::new();
    let mut test = new_test();
    let owner = Pubkey::new_unique();
    let mint_a = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mint_b = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let src_a = add_token(&mut test, mint_a, &owner, 100, true);
    let dst_a = add_token(&mut test, mint_a, &owner, 0, true);
    let mut ctx = test.start_with_context().await;
    for (mint, limit) in [(mint_a, 50), (mint_b, 7)] {
        init_hook(&mut ctx, mint, &ext, &max_args(limit))
            .await
            .unwrap();
    }
    let payer = ctx.payer.pubkey();
    let (config_a, config_b) = (
        config_address(&mint_a, &HOOK).0,
        config_address(&mint_b, &HOOK).0,
    );
    let (list_a, list_b) = (
        validation_list_address(&mint_a, &HOOK).0,
        validation_list_address(&mint_b, &HOOK).0,
    );
    let ix = |config, list| direct_execute(src_a, mint_a, dst_a, config, list, payer, 1);

    send(&mut ctx, &[ix(config_a, list_a)], &[]).await.unwrap();
    expect_hook_error(
        send(&mut ctx, &[ix(config_b, list_a)], &[]).await,
        HookError::InvalidConfigPda,
    );
    expect_hook_error(
        send(&mut ctx, &[ix(config_a, list_b)], &[]).await,
        HookError::InvalidValidationList,
    );
    expect_hook_error(
        send(&mut ctx, &[ix(config_b, list_b)], &[]).await,
        HookError::InvalidConfigPda,
    );
    // An arbitrary program-owned account that is not the config PDA.
    expect_hook_error(
        send(&mut ctx, &[ix(list_a, list_a)], &[]).await,
        HookError::InvalidConfigData,
    );
    // An account owned by somebody else in the config slot.
    expect_hook_error(
        send(&mut ctx, &[ix(src_a, list_a)], &[]).await,
        HookError::InvalidConfigOwner,
    );
}

#[tokio::test]
async fn execute_rejects_corrupt_config_and_list_without_panicking() {
    let mut test = new_test();
    let mut cases = Vec::new();
    let template = |mint: Pubkey| good_config(mint);
    let good_list = canonical_list();

    // Each case needs its own mint, so the config is built after the mint exists.
    macro_rules! case {
        ($expected:expr, $config:expr, $list:expr) => {{
            let owner = Pubkey::new_unique();
            let mint = add_mint(&mut test, &MintSpec::hooked(None, None));
            let config: Option<(Pubkey, Vec<u8>)> = ($config)(mint);
            let list: Option<(Pubkey, Vec<u8>)> = ($list)(mint);
            if let Some((config_owner, data)) = config {
                test.add_account(
                    config_address(&mint, &HOOK).0,
                    program_account(config_owner, data),
                );
            }
            if let Some((list_owner, data)) = list {
                test.add_account(
                    validation_list_address(&mint, &HOOK).0,
                    program_account(list_owner, data),
                );
            }
            cases.push(CorruptCase {
                mint,
                source: add_token(&mut test, mint, &owner, 100, true),
                destination: add_token(&mut test, mint, &owner, 0, true),
                expected: $expected,
            });
        }};
    }
    let no_list = |_: Pubkey| -> Option<(Pubkey, Vec<u8>)> { None };

    case!(
        HookError::InvalidConfigOwner,
        |m| Some((OTHER_PROGRAM, template(m).encode())),
        no_list
    );
    case!(
        HookError::InvalidConfigData,
        |m| Some((HOOK, template(m).encode()[..20].to_vec())),
        no_list
    );
    case!(
        HookError::InvalidConfigData,
        |m| {
            let mut data = template(m).encode();
            data.push(0);
            Some((HOOK, data))
        },
        no_list
    );
    case!(
        HookError::InvalidConfigData,
        |m| {
            let mut data = template(m).encode();
            data[0] = b'X';
            Some((HOOK, data))
        },
        no_list
    );
    case!(
        HookError::UnsupportedVersion,
        |m| {
            let mut data = template(m).encode();
            data[8] = 2;
            Some((HOOK, data))
        },
        no_list
    );
    case!(
        HookError::ParamsTooLarge,
        |m| {
            let mut data = template(m).encode();
            data[11..13].copy_from_slice(&u16::MAX.to_le_bytes());
            Some((HOOK, data))
        },
        no_list
    );
    // Config of another mint stored at this mint's PDA.
    case!(
        HookError::InvalidConfigPda,
        |_: Pubkey| Some((HOOK, template(Pubkey::new_unique()).encode())),
        no_list
    );
    // Wrong stored bump.
    case!(
        HookError::InvalidConfigPda,
        |m| {
            let mut config = template(m);
            config.bump = config.bump.wrapping_sub(1);
            Some((HOOK, config.encode()))
        },
        no_list
    );
    // Valid config, list problems.
    let valid = |m| Some((HOOK, template(m).encode()));
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| None);
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| Some((
        OTHER_PROGRAM,
        good_list.clone()
    )));
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| Some((
        HOOK,
        good_list[..10].to_vec()
    )));
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| Some((
        HOOK,
        Vec::new()
    )));
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| {
        let mut data = good_list.clone();
        data[0] ^= 1;
        Some((HOOK, data))
    });
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| {
        let mut data = good_list.clone();
        data[12..16].copy_from_slice(&0u32.to_le_bytes()); // zero entries
        Some((HOOK, data))
    });
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| {
        let mut data = good_list.clone();
        data[12..16].copy_from_slice(&u32::MAX.to_le_bytes()); // huge entry count
        Some((HOOK, data))
    });
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| {
        let mut data = good_list.clone();
        data.push(0); // trailing byte
        Some((HOOK, data))
    });
    // Wrong stored list bump.
    case!(
        HookError::InvalidValidationList,
        |m| {
            let mut config = template(m);
            config.list_bump = config.list_bump.wrapping_sub(1);
            Some((HOOK, config.encode()))
        },
        |_: Pubkey| Some((HOOK, good_list.clone()))
    );
    // Well-formed list whose meta is not the seeds-derived config meta.
    case!(HookError::InvalidValidationList, valid, |_: Pubkey| {
        let mut data = vec![0u8; VALIDATION_LIST_LEN];
        ExtraAccountMetaList::init::<ExecuteInstruction>(
            &mut data,
            &[
                spl_tlv_account_resolution::account::ExtraAccountMeta::new_with_pubkey(
                    &Pubkey::new_unique(),
                    false,
                    false,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        Some((HOOK, data))
    });

    let mut ctx = test.start_with_context().await;
    let payer = ctx.payer.pubkey();
    for case in cases {
        let instruction = direct_execute(
            case.source,
            case.mint,
            case.destination,
            config_address(&case.mint, &HOOK).0,
            validation_list_address(&case.mint, &HOOK).0,
            payer,
            1,
        );
        expect_hook_error(send(&mut ctx, &[instruction], &[]).await, case.expected);
    }
}

#[tokio::test]
async fn execute_requires_exactly_six_readonly_accounts() {
    let ext = Keypair::new();
    let mut test = new_test();
    let owner = Pubkey::new_unique();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let source = add_token(&mut test, mint, &owner, 100, true);
    let destination = add_token(&mut test, mint, &owner, 0, true);
    let mut ctx = test.start_with_context().await;
    init_hook(&mut ctx, mint, &ext, &max_args(50))
        .await
        .unwrap();
    let config = config_address(&mint, &HOOK).0;
    let list = validation_list_address(&mint, &HOOK).0;
    let payer = ctx.payer.pubkey();
    let good = direct_execute(source, mint, destination, config, list, payer, 1);
    assert_eq!(good.accounts.len(), 6);
    send(&mut ctx, std::slice::from_ref(&good), &[])
        .await
        .unwrap();

    let mut missing = good.clone();
    missing.accounts.pop();
    expect_hook_error(
        send(&mut ctx, &[missing], &[]).await,
        HookError::WrongAccountCount,
    );
    let mut trailing = good.clone();
    trailing
        .accounts
        .push(AccountMeta::new_readonly(Pubkey::new_unique(), false));
    expect_hook_error(
        send(&mut ctx, &[trailing], &[]).await,
        HookError::WrongAccountCount,
    );
    let mut duplicated = good.clone();
    duplicated.accounts.push(duplicated.accounts[5].clone());
    expect_hook_error(
        send(&mut ctx, &[duplicated], &[]).await,
        HookError::WrongAccountCount,
    );
    // Swapped order: config and list exchanged.
    let mut swapped = good.clone();
    swapped.accounts.swap(4, 5);
    expect_hook_error(
        send(&mut ctx, &[swapped], &[]).await,
        HookError::InvalidConfigData,
    );
    // Token-2022 never passes these accounts writable.
    for index in [0, 1, 2, 4, 5] {
        let mut writable = good.clone();
        writable.accounts[index].is_writable = true;
        expect_hook_error(
            send(&mut ctx, &[writable], &[]).await,
            HookError::AccountOrderMismatch,
        );
    }
    // Truncated or oversized instruction data.
    let mut short = good.clone();
    short.data.truncate(12);
    expect_instruction_error(
        send(&mut ctx, &[short], &[]).await,
        InstructionError::InvalidInstructionData,
    );
    let mut empty = good.clone();
    empty.data.clear();
    expect_instruction_error(
        send(&mut ctx, &[empty], &[]).await,
        InstructionError::InvalidInstructionData,
    );
    // The SPL list-management instructions are not aliased onto InitializeHook.
    let spl_init = spl_transfer_hook_interface::instruction::initialize_extra_account_meta_list(
        &HOOK,
        &list,
        &mint,
        &ext.pubkey(),
        &[],
    );
    expect_hook_error(
        send(&mut ctx, &[spl_init], &[&ext]).await,
        HookError::SplInterfaceUnsupported,
    );
    let spl_update = spl_transfer_hook_interface::instruction::update_extra_account_meta_list(
        &HOOK,
        &list,
        &mint,
        &ext.pubkey(),
        &[],
    );
    expect_hook_error(
        send(&mut ctx, &[spl_update], &[&ext]).await,
        HookError::SplInterfaceUnsupported,
    );
}

// ------------------------------------------------------------------------------------------
// Real Token-2022 transfers: boundaries and multi-mint
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn transfer_amount_boundaries_through_token_2022() {
    let ext = Keypair::new();
    let owner = Keypair::new();
    let mut test = new_test();
    let limited = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let unlimited = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let limited_src = add_token(&mut test, limited, &owner.pubkey(), u64::MAX, false);
    let limited_dst = add_token(&mut test, limited, &owner.pubkey(), 0, false);
    // Token-2022 checks the balance before calling the hook, so the u64::MAX case needs a
    // source that holds u64::MAX.
    let limited_full = add_token(&mut test, limited, &owner.pubkey(), u64::MAX, false);
    let limited_empty = add_token(&mut test, limited, &owner.pubkey(), 0, false);
    let unlimited_src = add_token(&mut test, unlimited, &owner.pubkey(), u64::MAX, false);
    let unlimited_dst = add_token(&mut test, unlimited, &owner.pubkey(), 0, false);
    let mut ctx = test.start_with_context().await;
    for (mint, limit) in [(limited, 50), (unlimited, u64::MAX)] {
        init_hook(&mut ctx, mint, &ext, &max_args(limit))
            .await
            .unwrap();
    }
    let transfer = |source, mint, destination, amount| {
        hooked_transfer(source, mint, destination, &owner.pubkey(), amount)
    };

    // amount == limit passes (inclusive), limit + 1 fails, u64::MAX fails.
    send(
        &mut ctx,
        &[transfer(limited_src, limited, limited_dst, 50)],
        &[&owner],
    )
    .await
    .expect("amount == limit must pass");
    assert_eq!(amount_of(&mut ctx, limited_dst).await, 50);
    for (source, destination, amount) in [
        (limited_src, limited_dst, 51),
        (limited_src, limited_dst, 1_000),
        (limited_full, limited_empty, u64::MAX),
    ] {
        expect_hook_error(
            send(
                &mut ctx,
                &[transfer(source, limited, destination, amount)],
                &[&owner],
            )
            .await,
            HookError::TransferExceedsLimit,
        );
    }
    assert_eq!(amount_of(&mut ctx, limited_dst).await, 50, "rollback");
    // A zero-amount transfer is within any limit.
    send(
        &mut ctx,
        &[transfer(limited_src, limited, limited_dst, 0)],
        &[&owner],
    )
    .await
    .unwrap();
    // limit == u64::MAX lets u64::MAX through.
    send(
        &mut ctx,
        &[transfer(unlimited_src, unlimited, unlimited_dst, u64::MAX)],
        &[&owner],
    )
    .await
    .expect("u64::MAX passes when limit == u64::MAX");
    assert_eq!(amount_of(&mut ctx, unlimited_dst).await, u64::MAX);
}

#[tokio::test]
async fn two_mints_under_one_program_enforce_their_own_limits() {
    let ext = Keypair::new();
    let owner = Keypair::new();
    let mut test = new_test();
    let small = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let large = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let small_src = add_token(&mut test, small, &owner.pubkey(), 1_000, false);
    let small_dst = add_token(&mut test, small, &owner.pubkey(), 0, false);
    let large_src = add_token(&mut test, large, &owner.pubkey(), 1_000, false);
    let large_dst = add_token(&mut test, large, &owner.pubkey(), 0, false);
    let mut ctx = test.start_with_context().await;
    for (mint, limit) in [(small, 10), (large, 100)] {
        init_hook(&mut ctx, mint, &ext, &max_args(limit))
            .await
            .unwrap();
    }
    assert_eq!(fetched_limit(&mut ctx, small).await, 10);
    assert_eq!(fetched_limit(&mut ctx, large).await, 100);

    send(
        &mut ctx,
        &[hooked_transfer(
            large_src,
            large,
            large_dst,
            &owner.pubkey(),
            50,
        )],
        &[&owner],
    )
    .await
    .unwrap();
    expect_hook_error(
        send(
            &mut ctx,
            &[hooked_transfer(
                small_src,
                small,
                small_dst,
                &owner.pubkey(),
                50,
            )],
            &[&owner],
        )
        .await,
        HookError::TransferExceedsLimit,
    );
    send(
        &mut ctx,
        &[hooked_transfer(
            small_src,
            small,
            small_dst,
            &owner.pubkey(),
            10,
        )],
        &[&owner],
    )
    .await
    .unwrap();
    // Config and list of the other mint cannot stand in: Token-2022 cannot resolve the extras.
    let mut wrong = hooked_transfer(small_src, small, small_dst, &owner.pubkey(), 50);
    wrong.accounts[4].pubkey = config_address(&large, &HOOK).0;
    assert!(send(&mut ctx, &[wrong], &[&owner]).await.is_err());
}
