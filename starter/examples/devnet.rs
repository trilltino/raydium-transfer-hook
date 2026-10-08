//! After the hook program is deployed: make a hooked mint, set the hook up for it, and prove the
//! rule on a real cluster (one transfer that must pass, one that must be refused).
//!
//! ```text
//! cargo run --example devnet -- \
//!     --url https://api.devnet.solana.com --keypair ~/.config/solana/id.json \
//!     --program-id <HOOK PROGRAM ID> [--limit 500]
//! ```
//!
//! `scripts/deploy.sh` runs this for you after it deploys the program. Change the setup
//! instruction below when you change the rule's parameters.

use std::{env, process::exit};

use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::InstructionError,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction,
    transaction::{Transaction, TransactionError},
};
use spl_token_2022::{
    extension::{transfer_hook, ExtensionType},
    instruction as token_instruction,
    offchain::{create_transfer_checked_instruction_with_extra_metas, AccountFetchError},
    state::{Account as TokenAccount, Mint},
};
use transfer_hook_starter::{
    config_address, initialize_hook_instruction, validation_list_address, AuthorityMode, HookError,
    InitializeHookArgs,
};

const DECIMALS: u8 = 6;

struct Args {
    url: String,
    keypair: String,
    program_id: Pubkey,
    limit: u64,
}

fn parse_args() -> Args {
    let mut url = "https://api.devnet.solana.com".to_string();
    let mut keypair = None;
    let mut program_id = None;
    let mut limit = 500u64;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .unwrap_or_else(|| fail(&format!("{flag} needs a value")));
        match flag.as_str() {
            "--url" => url = value,
            "--keypair" => keypair = Some(value),
            "--program-id" => {
                program_id = Some(value.parse().unwrap_or_else(|_| fail("bad --program-id")))
            }
            "--limit" => limit = value.parse().unwrap_or_else(|_| fail("bad --limit")),
            other => fail(&format!("unknown flag {other}")),
        }
    }
    Args {
        url,
        keypair: keypair.unwrap_or_else(|| fail("--keypair is required")),
        program_id: program_id.unwrap_or_else(|| fail("--program-id is required")),
        limit,
    }
}

fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    exit(1);
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let payer = read_keypair_file(&args.keypair)
        .unwrap_or_else(|error| fail(&format!("cannot read {}: {error}", args.keypair)));
    let rpc = RpcClient::new_with_commitment(args.url.clone(), CommitmentConfig::confirmed());
    let token_program = spl_token_2022::id();

    let program = rpc
        .get_account(&args.program_id)
        .await
        .unwrap_or_else(|error| {
            fail(&format!(
                "hook program {} not found: {error}",
                args.program_id
            ))
        });
    if !program.executable {
        fail("the program id is not an executable program; deploy it first");
    }

    // 8-10. A Token-2022 mint whose TransferHook points at the program, and the hook's own
    // per-mint config + validation list (ExtraAccountMetaList), created together.
    let mint = Keypair::new();
    let mint_len =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook]).unwrap();
    let mint_rent = rpc
        .get_minimum_balance_for_rent_exemption(mint_len)
        .await
        .unwrap();
    let setup = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            mint_rent,
            mint_len as u64,
            &token_program,
        ),
        transfer_hook::instruction::initialize(
            &token_program,
            &mint.pubkey(),
            Some(payer.pubkey()),
            Some(args.program_id),
        )
        .unwrap(),
        token_instruction::initialize_mint2(
            &token_program,
            &mint.pubkey(),
            &payer.pubkey(),
            None,
            DECIMALS,
        )
        .unwrap(),
        initialize_hook_instruction(
            args.program_id,
            mint.pubkey(),
            payer.pubkey(),
            payer.pubkey(),
            &InitializeHookArgs::max_transfer(
                AuthorityMode::ExtensionAuthority,
                args.limit,
                Pubkey::default(),
            ),
        ),
    ];
    send(&rpc, &payer, &setup, &[&mint]).await;

    // Two token accounts and a supply of four times the limit.
    let account_len = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
        ExtensionType::TransferHookAccount,
    ])
    .unwrap();
    let account_rent = rpc
        .get_minimum_balance_for_rent_exemption(account_len)
        .await
        .unwrap();
    let (source, destination) = (Keypair::new(), Keypair::new());
    let mut instructions = Vec::new();
    for account in [&source, &destination] {
        instructions.push(system_instruction::create_account(
            &payer.pubkey(),
            &account.pubkey(),
            account_rent,
            account_len as u64,
            &token_program,
        ));
        instructions.push(
            token_instruction::initialize_account3(
                &token_program,
                &account.pubkey(),
                &mint.pubkey(),
                &payer.pubkey(),
            )
            .unwrap(),
        );
    }
    instructions.push(
        token_instruction::mint_to_checked(
            &token_program,
            &mint.pubkey(),
            &source.pubkey(),
            &payer.pubkey(),
            &[],
            args.limit * 4,
            DECIMALS,
        )
        .unwrap(),
    );
    send(&rpc, &payer, &instructions, &[&source, &destination]).await;

    // 11-12. A transfer at the limit must pass; one unit over must be refused by the hook.
    let at_limit = transfer(&rpc, &payer, &source, &destination, &mint, args.limit).await;
    send(&rpc, &payer, &[at_limit], &[]).await;
    println!("PASS  transfer of {} (= limit) allowed", args.limit);

    let over = transfer(&rpc, &payer, &source, &destination, &mint, args.limit + 1).await;
    let refused = rpc
        .simulate_transaction(&Transaction::new_signed_with_payer(
            &[over],
            Some(&payer.pubkey()),
            &[&payer],
            rpc.get_latest_blockhash().await.unwrap(),
        ))
        .await
        .unwrap()
        .value;
    let expected = HookError::TransferExceedsLimit.code();
    match refused.err {
        Some(TransactionError::InstructionError(_, InstructionError::Custom(code)))
            if code == expected =>
        {
            println!(
                "PASS  transfer of {} (limit + 1) refused by the hook (custom error {code:#x})",
                args.limit + 1
            );
        }
        other => fail(&format!(
            "expected the hook to refuse with {expected:#x}, got {other:?}\n{:#?}",
            refused.logs
        )),
    }

    println!("\ncluster            {}", args.url);
    println!("hook program id    {}", args.program_id);
    println!("mint               {}", mint.pubkey());
    println!(
        "hook config PDA    {}",
        config_address(&mint.pubkey(), &args.program_id).0
    );
    println!(
        "validation list    {}",
        validation_list_address(&mint.pubkey(), &args.program_id).0
    );
    println!("source account     {}", source.pubkey());
    println!("destination        {}", destination.pubkey());
    println!(
        "\nnext: change the rule in src/rule.rs and redeploy with the same --program-id, or read \
         the mint's TransferHook authority and upgrade authority before you tell anyone it is \"safe\"."
    );
}

async fn transfer(
    rpc: &RpcClient,
    payer: &Keypair,
    source: &Keypair,
    destination: &Keypair,
    mint: &Keypair,
    amount: u64,
) -> solana_sdk::instruction::Instruction {
    create_transfer_checked_instruction_with_extra_metas(
        &spl_token_2022::id(),
        &source.pubkey(),
        &mint.pubkey(),
        &destination.pubkey(),
        &payer.pubkey(),
        &[],
        amount,
        DECIMALS,
        |key| async move {
            Ok::<_, AccountFetchError>(
                rpc.get_account_with_commitment(&key, CommitmentConfig::confirmed())
                    .await
                    .map_err(|error| -> AccountFetchError { Box::new(error) })?
                    .value
                    .map(|account| account.data),
            )
        },
    )
    .await
    .unwrap_or_else(|error| fail(&format!("could not resolve the hook's accounts: {error}")))
}

async fn send(
    rpc: &RpcClient,
    payer: &Keypair,
    instructions: &[solana_sdk::instruction::Instruction],
    extra_signers: &[&Keypair],
) {
    let mut signers: Vec<&Keypair> = vec![payer];
    signers.extend_from_slice(extra_signers);
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &signers,
        rpc.get_latest_blockhash().await.unwrap(),
    );
    if let Err(error) = rpc.send_and_confirm_transaction(&transaction).await {
        fail(&format!("transaction failed: {error}"));
    }
}
