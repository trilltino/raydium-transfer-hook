//! Hook-program behavior tests. Every failure asserts the exact error code.
//!
//! Mints and token accounts are pre-loaded as Token-2022 owned account data so that guard paths
//! (for example a forged `transferring` flag, which only Token-2022 can set on-chain) can be
//! exercised directly. Transfers that go through the real Token-2022 program use the same
//! pre-loaded accounts. The hook runs natively unless `SBF_OUT_DIR` points at a directory
//! holding `reference_hook_onchain.so`, in which case ProgramTest runs the SBF build.

use {
    reference_hook_onchain::{
        config_address, config_extra_account_meta, initialize_hook_instruction,
        process_instruction, set_config_authority_instruction, update_config_instruction,
        validation_list_address, AuthorityMode, HookConfig, HookError, InitializeHookArgs,
        MAX_PARAMS_LEN, TEMPLATE_MAX_TRANSFER_V1, VALIDATION_LIST_LEN,
    },
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
        "reference_hook_onchain",
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

async fn fetch_config(ctx: &mut ProgramTestContext, mint: Pubkey) -> HookConfig {
    let account = fetch(ctx, config_address(&mint, &HOOK).0)
        .await
        .expect("config account exists");
    assert_eq!(account.owner, HOOK);
    HookConfig::decode(&account.data).expect("config decodes strictly")
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

fn max_args(mode: AuthorityMode, limit: u64, config_authority: Pubkey) -> InitializeHookArgs {
    InitializeHookArgs::max_transfer(mode, limit, config_authority)
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
    let args = max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default());

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
// InitializeHook: authority modes (F3)
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn initialize_enforces_the_authority_required_by_each_mode() {
    let ext = Keypair::new();
    let mint_auth = Keypair::new();
    let stranger = Keypair::new();
    let explicit = Keypair::new();
    let mut test = new_test();
    let good = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
    );
    let no_ext = add_mint(&mut test, &MintSpec::hooked(None, Some(mint_auth.pubkey())));
    let no_mint_auth = add_mint(&mut test, &MintSpec::hooked(Some(ext.pubkey()), None));
    let ok = [
        (
            AuthorityMode::ExtensionAuthority,
            add_mint(
                &mut test,
                &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
            ),
        ),
        (
            AuthorityMode::MintAuthority,
            add_mint(
                &mut test,
                &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
            ),
        ),
        (
            AuthorityMode::Explicit,
            add_mint(
                &mut test,
                &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
            ),
        ),
        (
            AuthorityMode::Immutable,
            add_mint(
                &mut test,
                &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
            ),
        ),
    ];
    let mut ctx = test.start_with_context().await;
    let zero = Pubkey::default();

    // Mode 0: live extension authority.
    let args = max_args(AuthorityMode::ExtensionAuthority, 50, zero);
    expect_hook_error(
        init_hook(&mut ctx, good, &stranger, &args).await,
        HookError::AuthorityMismatch,
    );
    expect_hook_error(
        init_hook(&mut ctx, good, &mint_auth, &args).await,
        HookError::AuthorityMismatch,
    );
    expect_hook_error(
        init_hook(&mut ctx, no_ext, &ext, &args).await,
        HookError::AuthorityUnavailable,
    );
    // Mode 1: live mint authority; the extension authority is not enough.
    let args = max_args(AuthorityMode::MintAuthority, 50, zero);
    expect_hook_error(
        init_hook(&mut ctx, good, &ext, &args).await,
        HookError::AuthorityMismatch,
    );
    expect_hook_error(
        init_hook(&mut ctx, no_mint_auth, &ext, &args).await,
        HookError::AuthorityUnavailable,
    );
    // Mode 2: extension authority consents, an explicit non-zero config authority is stored.
    let args = max_args(AuthorityMode::Explicit, 50, explicit.pubkey());
    expect_hook_error(
        init_hook(&mut ctx, good, &stranger, &args).await,
        HookError::AuthorityMismatch,
    );
    expect_hook_error(
        init_hook(&mut ctx, no_ext, &ext, &args).await,
        HookError::AuthorityUnavailable,
    );
    expect_hook_error(
        init_hook(
            &mut ctx,
            good,
            &ext,
            &max_args(AuthorityMode::Explicit, 50, zero),
        )
        .await,
        HookError::AuthorityUnavailable,
    );
    // Mode 3: needs the extension authority to consent.
    expect_hook_error(
        init_hook(
            &mut ctx,
            no_ext,
            &ext,
            &max_args(AuthorityMode::Immutable, 50, zero),
        )
        .await,
        HookError::AuthorityUnavailable,
    );
    // A stored config authority is only meaningful in mode 2.
    for mode in [
        AuthorityMode::ExtensionAuthority,
        AuthorityMode::MintAuthority,
        AuthorityMode::Immutable,
    ] {
        let signer = if mode == AuthorityMode::MintAuthority {
            &mint_auth
        } else {
            &ext
        };
        expect_hook_error(
            init_hook(
                &mut ctx,
                good,
                signer,
                &max_args(mode, 50, explicit.pubkey()),
            )
            .await,
            HookError::InvalidParams,
        );
    }
    // Mode 4 (PlatformControlled) is reserved; anything above is unknown.
    for mode in [4u8, 5, 255] {
        let mut args = max_args(AuthorityMode::ExtensionAuthority, 50, zero);
        args.authority_mode = mode;
        expect_hook_error(
            init_hook(&mut ctx, good, &ext, &args).await,
            HookError::UnsupportedMode,
        );
    }
    // The authority must actually sign.
    let mut unsigned = initialize_hook_instruction(
        HOOK,
        good,
        ext.pubkey(),
        ctx.payer.pubkey(),
        &max_args(AuthorityMode::ExtensionAuthority, 50, zero),
    );
    unsigned.accounts[3].is_signer = false;
    expect_instruction_error(
        send(&mut ctx, &[unsigned], &[]).await,
        InstructionError::MissingRequiredSignature,
    );

    // Success path for every mode, then check the stored state.
    for (mode, mint) in ok {
        let signer = if mode == AuthorityMode::MintAuthority {
            &mint_auth
        } else {
            &ext
        };
        let config_authority = if mode == AuthorityMode::Explicit {
            explicit.pubkey()
        } else {
            zero
        };
        init_hook(
            &mut ctx,
            mint,
            signer,
            &max_args(mode, 1234, config_authority),
        )
        .await
        .unwrap();
        let config = fetch_config(&mut ctx, mint).await;
        assert_eq!(config.authority_mode, mode);
        assert_eq!(config.config_authority, config_authority);
        assert_eq!(config.mint, mint);
        assert_eq!(config.config_seq, 0);
        assert_eq!(config.template_id, TEMPLATE_MAX_TRANSFER_V1);
        assert_eq!(config.max_transfer_limit().unwrap(), 1234);
        let (address, bump) = config_address(&mint, &HOOK);
        assert_eq!(config.bump, bump);
        config.verify_address(&HOOK, &mint, &address).unwrap();
        assert_eq!(config.list_bump, validation_list_address(&mint, &HOOK).1);
    }
}

#[tokio::test]
async fn initialize_rejects_invalid_template_params_and_accounts() {
    let ext = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mut ctx = test.start_with_context().await;
    let base = max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default());

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
    let mut args = base.clone();
    args.template_id = [9; 32];
    expect_hook_error(
        init_hook(&mut ctx, mint, &ext, &args).await,
        HookError::UnknownTemplate,
    );
    let mut args = base.clone();
    args.template_version = 2;
    expect_hook_error(
        init_hook(&mut ctx, mint, &ext, &args).await,
        HookError::UnsupportedVersion,
    );
    let mut args = base.clone();
    args.flags = 1;
    expect_hook_error(
        init_hook(&mut ctx, mint, &ext, &args).await,
        HookError::InvalidParams,
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
    instruction.data.truncate(40);
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
    init_hook(
        &mut ctx,
        mint,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    let before = fetch(&mut ctx, config_address(&mint, &HOOK).0)
        .await
        .unwrap();
    for args in [
        max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
        max_args(AuthorityMode::ExtensionAuthority, 9_999, Pubkey::default()),
        max_args(AuthorityMode::Immutable, 9_999, Pubkey::default()),
    ] {
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
    let config_rent = rent.minimum_balance(264);
    let list_rent = rent.minimum_balance(VALIDATION_LIST_LEN);
    assert_eq!(config_rent, 2_728_320);

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

        init_hook(
            &mut ctx,
            mint,
            &ext,
            &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
        )
        .await
        .expect("pre-funded PDAs must not block initialization");

        let config = fetch(&mut ctx, config_key).await.unwrap();
        let list = fetch(&mut ctx, list_key).await.unwrap();
        assert_eq!(config.owner, HOOK);
        assert_eq!(list.owner, HOOK);
        assert_eq!(config.data.len(), 264);
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
        init_hook(
            &mut ctx,
            *mint,
            &ext,
            &max_args(
                AuthorityMode::ExtensionAuthority,
                10 + index as u64,
                Pubkey::default(),
            ),
        )
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

fn good_config(mint: Pubkey) -> HookConfig {
    HookConfig::new(
        config_address(&mint, &HOOK).1,
        validation_list_address(&mint, &HOOK).1,
        AuthorityMode::ExtensionAuthority,
        TEMPLATE_MAX_TRANSFER_V1,
        1,
        mint,
        Pubkey::default(),
        0,
        &50u64.to_le_bytes(),
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
    init_hook(
        &mut ctx,
        mint,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
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
        init_hook(
            &mut ctx,
            mint,
            &ext,
            &max_args(AuthorityMode::ExtensionAuthority, limit, Pubkey::default()),
        )
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
        |m| Some((HOOK, template(m).encode()[..100].to_vec())),
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
        HookError::HashMismatch,
        |m| {
            let mut data = template(m).encode();
            data[256] ^= 1;
            Some((HOOK, data))
        },
        no_list
    );
    case!(
        HookError::InvalidConfigData,
        |m| {
            let mut data = template(m).encode();
            data[200] = 1; // reserved bytes must be zero
            Some((HOOK, data))
        },
        no_list
    );
    case!(
        HookError::UnsupportedMode,
        |m| {
            let mut data = template(m).encode();
            data[11] = 4;
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
    case!(HookError::AccountOrderMismatch, valid, |_: Pubkey| {
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
    init_hook(
        &mut ctx,
        mint,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    let config = config_address(&mint, &HOOK).0;
    let list = validation_list_address(&mint, &HOOK).0;
    let payer = ctx.payer.pubkey();
    let good = direct_execute(source, mint, destination, config, list, payer, 1);
    assert_eq!(good.accounts.len(), 6);
    send(&mut ctx, &[good.clone()], &[]).await.unwrap();

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
        init_hook(
            &mut ctx,
            mint,
            &ext,
            &max_args(AuthorityMode::ExtensionAuthority, limit, Pubkey::default()),
        )
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
        init_hook(
            &mut ctx,
            mint,
            &ext,
            &max_args(AuthorityMode::ExtensionAuthority, limit, Pubkey::default()),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        fetch_config(&mut ctx, small)
            .await
            .max_transfer_limit()
            .unwrap(),
        10
    );
    assert_eq!(
        fetch_config(&mut ctx, large)
            .await
            .max_transfer_limit()
            .unwrap(),
        100
    );

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

#[tokio::test]
async fn execute_compute_units_are_reported() {
    let ext = Keypair::new();
    let owner = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let source = add_token(&mut test, mint, &owner.pubkey(), 100, false);
    let destination = add_token(&mut test, mint, &owner.pubkey(), 0, false);
    let mut ctx = test.start_with_context().await;
    init_hook(
        &mut ctx,
        mint,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    let blockhash = ctx.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[hooked_transfer(
            source,
            mint,
            destination,
            &owner.pubkey(),
            10,
        )],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, &owner],
        blockhash,
    );
    let outcome = ctx.banks_client.simulate_transaction(tx).await.unwrap();
    outcome.result.unwrap().unwrap();
    let logs = outcome.simulation_details.unwrap().logs;
    let invoke = format!("Program {HOOK} invoke [");
    let consumed = format!("Program {HOOK} consumed ");
    let units = logs
        .iter()
        .find_map(|l| l.strip_prefix(&consumed))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse::<u64>().ok());
    // Only the SBF build (SBF_OUT_DIR set) has a meaningful compute figure and one log line per
    // invocation; the native builtin processor duplicates invoke logs and reports no usage.
    if std::env::var_os("SBF_OUT_DIR").is_some() {
        assert_eq!(
            logs.iter().filter(|l| l.starts_with(&invoke)).count(),
            1,
            "{logs:#?}"
        );
        let units = units.expect("hook consumed-units log line");
        println!("HOOK_EXECUTE_COMPUTE_UNITS={units}");
        assert!(units < 25_000, "Execute used {units} CU");
    } else {
        assert!(logs.iter().any(|l| l.starts_with(&invoke)), "{logs:#?}");
    }
    assert!(
        logs.iter().all(|l| !l.contains("Transfer Hook Execute")),
        "Execute must not log"
    );
}

// ------------------------------------------------------------------------------------------
// UpdateConfig and SetConfigAuthority
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn update_config_requires_current_seq_and_the_mode_authority() {
    let ext = Keypair::new();
    let mint_auth = Keypair::new();
    let stranger = Keypair::new();
    let owner = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
    );
    let source = add_token(&mut test, mint, &owner.pubkey(), 1_000, false);
    let destination = add_token(&mut test, mint, &owner.pubkey(), 0, false);
    let mut ctx = test.start_with_context().await;
    init_hook(
        &mut ctx,
        mint,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    let update = |authority: &Keypair, seq: u64, limit: u64| {
        update_config_instruction(
            HOOK,
            mint,
            authority.pubkey(),
            seq,
            1,
            0,
            &limit.to_le_bytes(),
        )
    };

    expect_hook_error(
        send(&mut ctx, &[update(&stranger, 0, 500)], &[&stranger]).await,
        HookError::AuthorityMismatch,
    );
    // Mode 0 follows the extension authority, not the mint authority.
    expect_hook_error(
        send(&mut ctx, &[update(&mint_auth, 0, 500)], &[&mint_auth]).await,
        HookError::AuthorityMismatch,
    );
    let mut unsigned = update(&ext, 0, 500);
    unsigned.accounts[2].is_signer = false;
    expect_instruction_error(
        send(&mut ctx, &[unsigned], &[]).await,
        InstructionError::MissingRequiredSignature,
    );
    expect_hook_error(
        send(&mut ctx, &[update(&ext, 1, 500)], &[&ext]).await,
        HookError::StaleConfigSeq,
    );
    expect_hook_error(
        send(&mut ctx, &[update(&ext, 0, 0)], &[&ext]).await,
        HookError::InvalidParams,
    );
    let mut bad_version = update(&ext, 0, 500);
    bad_version.data[16..20].copy_from_slice(&2u32.to_le_bytes());
    expect_hook_error(
        send(&mut ctx, &[bad_version], &[&ext]).await,
        HookError::UnsupportedVersion,
    );
    let mut bad_flags = update(&ext, 0, 500);
    bad_flags.data[20..28].copy_from_slice(&1u64.to_le_bytes());
    expect_hook_error(
        send(&mut ctx, &[bad_flags], &[&ext]).await,
        HookError::InvalidParams,
    );
    // The config of another mint cannot be substituted.
    let mut substituted = update(&ext, 0, 500);
    substituted.accounts[0].pubkey = config_address(&Pubkey::new_unique(), &HOOK).0;
    expect_instruction_error(
        send(&mut ctx, &[substituted], &[&ext]).await,
        InstructionError::Custom(HookError::InvalidConfigOwner.code()),
    );
    // Nothing changed so far.
    let config = fetch_config(&mut ctx, mint).await;
    assert_eq!(
        (config.config_seq, config.max_transfer_limit().unwrap()),
        (0, 50)
    );

    // Rejected at limit 50 before the update, accepted after it.
    expect_hook_error(
        send(
            &mut ctx,
            &[hooked_transfer(
                source,
                mint,
                destination,
                &owner.pubkey(),
                200,
            )],
            &[&owner],
        )
        .await,
        HookError::TransferExceedsLimit,
    );
    send(&mut ctx, &[update(&ext, 0, 500)], &[&ext])
        .await
        .unwrap();
    let config = fetch_config(&mut ctx, mint).await;
    assert_eq!(
        (config.config_seq, config.max_transfer_limit().unwrap()),
        (1, 500)
    );
    assert_eq!(
        config.config_hash,
        reference_hook_onchain::compute_config_hash(
            &TEMPLATE_MAX_TRANSFER_V1,
            1,
            &500u64.to_le_bytes()
        )
    );
    send(
        &mut ctx,
        &[hooked_transfer(
            source,
            mint,
            destination,
            &owner.pubkey(),
            200,
        )],
        &[&owner],
    )
    .await
    .unwrap();
    // The old sequence number is now stale.
    expect_hook_error(
        send(&mut ctx, &[update(&ext, 0, 600)], &[&ext]).await,
        HookError::StaleConfigSeq,
    );
    send(&mut ctx, &[update(&ext, 1, 600)], &[&ext])
        .await
        .unwrap();
    assert_eq!(fetch_config(&mut ctx, mint).await.config_seq, 2);
}

#[tokio::test]
async fn update_authority_follows_the_live_mint_state_in_modes_0_and_1() {
    let ext = Keypair::new();
    let new_ext = Keypair::new();
    let mint_auth = Keypair::new();
    let mut test = new_test();
    let mode0 = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
    );
    let mode1 = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(mint_auth.pubkey())),
    );
    let mut ctx = test.start_with_context().await;
    init_hook(
        &mut ctx,
        mode0,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    init_hook(
        &mut ctx,
        mode1,
        &mint_auth,
        &max_args(AuthorityMode::MintAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    let update = |mint, authority: &Keypair, seq| {
        update_config_instruction(
            HOOK,
            mint,
            authority.pubkey(),
            seq,
            1,
            0,
            &70u64.to_le_bytes(),
        )
    };

    // Mode 1: only the mint authority.
    expect_hook_error(
        send(&mut ctx, &[update(mode1, &ext, 0)], &[&ext]).await,
        HookError::AuthorityMismatch,
    );
    send(&mut ctx, &[update(mode1, &mint_auth, 0)], &[&mint_auth])
        .await
        .unwrap();

    // The extension authority rotates: mode 0 follows the new key.
    let mut spec = MintSpec::hooked(Some(new_ext.pubkey()), Some(mint_auth.pubkey()));
    ctx.set_account(&mode0, &AccountSharedData::from(spec.account()));
    expect_hook_error(
        send(&mut ctx, &[update(mode0, &ext, 0)], &[&ext]).await,
        HookError::AuthorityMismatch,
    );
    send(&mut ctx, &[update(mode0, &new_ext, 0)], &[&new_ext])
        .await
        .unwrap();
    // The extension authority is revoked: nobody can update (the config stays usable).
    spec.ext_authority = None;
    ctx.set_account(&mode0, &AccountSharedData::from(spec.account()));
    expect_hook_error(
        send(&mut ctx, &[update(mode0, &new_ext, 1)], &[&new_ext]).await,
        HookError::AuthorityUnavailable,
    );
    // The mint moves to another hook program: updates are refused.
    spec.ext_authority = Some(new_ext.pubkey());
    spec.hook_program = Some(OTHER_PROGRAM);
    ctx.set_account(&mode0, &AccountSharedData::from(spec.account()));
    expect_hook_error(
        send(&mut ctx, &[update(mode0, &new_ext, 1)], &[&new_ext]).await,
        HookError::MintHookProgramMismatch,
    );
    // The mint is no longer a Token-2022 mint at all.
    spec.hook_program = Some(HOOK);
    spec.owner = OTHER_PROGRAM;
    ctx.set_account(&mode0, &AccountSharedData::from(spec.account()));
    expect_hook_error(
        send(&mut ctx, &[update(mode0, &new_ext, 1)], &[&new_ext]).await,
        HookError::MintOwnerNotToken2022,
    );
}

#[tokio::test]
async fn explicit_authority_can_be_rotated_and_frozen_one_way() {
    let ext = Keypair::new();
    let explicit = Keypair::new();
    let next = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mode0 = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let immutable = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mut ctx = test.start_with_context().await;
    init_hook(
        &mut ctx,
        mint,
        &ext,
        &max_args(AuthorityMode::Explicit, 50, explicit.pubkey()),
    )
    .await
    .unwrap();
    init_hook(
        &mut ctx,
        mode0,
        &ext,
        &max_args(AuthorityMode::ExtensionAuthority, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    init_hook(
        &mut ctx,
        immutable,
        &ext,
        &max_args(AuthorityMode::Immutable, 50, Pubkey::default()),
    )
    .await
    .unwrap();
    let update = |mint, authority: &Keypair, seq| {
        update_config_instruction(
            HOOK,
            mint,
            authority.pubkey(),
            seq,
            1,
            0,
            &70u64.to_le_bytes(),
        )
    };
    let set = |mint, authority: &Keypair, new| {
        set_config_authority_instruction(HOOK, mint, authority.pubkey(), new)
    };

    // Mode 2: the stored authority updates; the extension authority (who consented) cannot.
    expect_hook_error(
        send(&mut ctx, &[update(mint, &ext, 0)], &[&ext]).await,
        HookError::AuthorityMismatch,
    );
    send(&mut ctx, &[update(mint, &explicit, 0)], &[&explicit])
        .await
        .unwrap();
    // Rotate the authority.
    expect_hook_error(
        send(&mut ctx, &[set(mint, &ext, next.pubkey())], &[&ext]).await,
        HookError::AuthorityMismatch,
    );
    send(
        &mut ctx,
        &[set(mint, &explicit, next.pubkey())],
        &[&explicit],
    )
    .await
    .unwrap();
    let config = fetch_config(&mut ctx, mint).await;
    assert_eq!(config.config_authority, next.pubkey());
    assert_eq!(config.authority_mode, AuthorityMode::Explicit);
    assert_eq!(config.config_seq, 2);
    expect_hook_error(
        send(&mut ctx, &[update(mint, &explicit, 2)], &[&explicit]).await,
        HookError::AuthorityMismatch,
    );
    send(&mut ctx, &[update(mint, &next, 2)], &[&next])
        .await
        .unwrap();
    // A zero authority is the one-way transition to Immutable.
    send(&mut ctx, &[set(mint, &next, Pubkey::default())], &[&next])
        .await
        .unwrap();
    let config = fetch_config(&mut ctx, mint).await;
    assert_eq!(config.authority_mode, AuthorityMode::Immutable);
    assert_eq!(config.config_authority, Pubkey::default());
    assert_eq!(config.config_seq, 4);
    for signer in [&next, &ext, &explicit] {
        expect_hook_error(
            send(&mut ctx, &[update(mint, signer, 4)], &[signer]).await,
            HookError::ConfigImmutable,
        );
        expect_hook_error(
            send(&mut ctx, &[set(mint, signer, signer.pubkey())], &[signer]).await,
            HookError::ConfigImmutable,
        );
    }

    // Immutable from the start: update and set both fail.
    expect_hook_error(
        send(&mut ctx, &[update(immutable, &ext, 0)], &[&ext]).await,
        HookError::ConfigImmutable,
    );
    expect_hook_error(
        send(&mut ctx, &[set(immutable, &ext, next.pubkey())], &[&ext]).await,
        HookError::ConfigImmutable,
    );
    // SetConfigAuthority is only for mode 2.
    expect_hook_error(
        send(&mut ctx, &[set(mode0, &ext, next.pubkey())], &[&ext]).await,
        HookError::UnsupportedMode,
    );
    assert_eq!(fetch_config(&mut ctx, mode0).await.config_seq, 0);
}

#[tokio::test]
async fn config_seq_overflow_is_an_error_not_a_wrap() {
    let ext = Keypair::new();
    let mut test = new_test();
    let mint = add_mint(
        &mut test,
        &MintSpec::hooked(Some(ext.pubkey()), Some(ext.pubkey())),
    );
    let mut config = good_config(mint);
    config.config_seq = u64::MAX;
    test.add_account(
        config_address(&mint, &HOOK).0,
        program_account(HOOK, config.encode()),
    );
    let mut ctx = test.start_with_context().await;
    let instruction = update_config_instruction(
        HOOK,
        mint,
        ext.pubkey(),
        u64::MAX,
        1,
        0,
        &70u64.to_le_bytes(),
    );
    expect_instruction_error(
        send(&mut ctx, &[instruction], &[&ext]).await,
        InstructionError::ArithmeticOverflow,
    );
}
