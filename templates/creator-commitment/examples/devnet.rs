//! After the hook program is deployed: make a hooked mint, commit a creator account to a vesting
//! schedule, and prove the floor on a real cluster (one transfer down to the floor must pass, one
//! token below it must be refused).
//!
//! ```text
//! cargo run --example devnet -- \
//!     --url https://api.devnet.solana.com --keypair <DEVNET KEYPAIR> \
//!     --program-id <HOOK PROGRAM ID> [--locked 600]
//! ```
//!
//! `scripts/deploy.sh templates/creator-commitment` runs this after it deploys the program. The
//! schedule's cliff is a day away, so the whole `--locked` amount stays locked during the run
//! whatever the cluster clock says.

use std::{
    env,
    process::exit,
    time::{SystemTime, UNIX_EPOCH},
};

use creator_commitment_hook::{
    config::config_address, error::CommitmentError, instruction::initialize, rule::Schedule,
};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{Instruction, InstructionError},
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

const DECIMALS: u8 = 6;
/// What the creator account holds.
const SUPPLY: u64 = 1_000;
const DAY: i64 = 86_400;

struct Args {
    url: String,
    keypair: String,
    program_id: Pubkey,
    locked: u64,
}

fn parse_args() -> Args {
    let mut url = "https://api.devnet.solana.com".to_string();
    let mut keypair = None;
    let mut program_id = None;
    let mut locked = 600u64;
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
            "--locked" => locked = value.parse().unwrap_or_else(|_| fail("bad --locked")),
            other => fail(&format!("unknown flag {other}")),
        }
    }
    if locked == 0 || locked >= SUPPLY {
        fail(&format!("--locked must be between 1 and {}", SUPPLY - 1));
    }
    Args {
        url,
        keypair: keypair.unwrap_or_else(|| fail("--keypair is required")),
        program_id: program_id.unwrap_or_else(|| fail("--program-id is required")),
        locked,
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

    // A Token-2022 mint whose TransferHook points at the program (the payer is its hook
    // authority), the creator's account and a recipient. Minting is not a transfer, so it works
    // before the hook is initialised.
    let mint = Keypair::new();
    let mint_len =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook]).unwrap();
    let account_len = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
        ExtensionType::TransferHookAccount,
    ])
    .unwrap();
    let mint_rent = rent(&rpc, mint_len).await;
    let account_rent = rent(&rpc, account_len).await;
    let (creator, recipient) = (Keypair::new(), Keypair::new());
    let mut setup = vec![
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
    ];
    for account in [&creator, &recipient] {
        setup.push(system_instruction::create_account(
            &payer.pubkey(),
            &account.pubkey(),
            account_rent,
            account_len as u64,
            &token_program,
        ));
        setup.push(
            token_instruction::initialize_account3(
                &token_program,
                &account.pubkey(),
                &mint.pubkey(),
                &payer.pubkey(),
            )
            .unwrap(),
        );
    }
    setup.push(
        token_instruction::mint_to_checked(
            &token_program,
            &mint.pubkey(),
            &creator.pubkey(),
            &payer.pubkey(),
            &[],
            SUPPLY,
            DECIMALS,
        )
        .unwrap(),
    );
    send(&rpc, &payer, &setup, &[&mint, &creator, &recipient]).await;

    // Commit the creator account: everything locked until a cliff a day from now.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_else(|_| fail("system clock is before 1970"));
    let schedule = Schedule {
        locked_total: args.locked,
        start: now - DAY,
        cliff: now + DAY,
        end: now + 2 * DAY,
    };
    let commit = initialize(
        &args.program_id,
        &payer.pubkey(),
        &payer.pubkey(),
        &mint.pubkey(),
        &creator.pubkey(),
        schedule,
    );
    send(&rpc, &payer, &[commit], &[]).await;

    // Down to exactly the floor must pass; one token below it must be refused by the hook.
    let free = SUPPLY - args.locked;
    let to_floor = transfer(&rpc, &payer, &creator, &recipient, &mint, free).await;
    send(&rpc, &payer, &[to_floor], &[]).await;
    println!(
        "PASS  transfer of {free} (down to the floor of {}) allowed",
        args.locked
    );

    let below = transfer(&rpc, &payer, &creator, &recipient, &mint, 1).await;
    let refused = rpc
        .simulate_transaction(&Transaction::new_signed_with_payer(
            &[below],
            Some(&payer.pubkey()),
            &[&payer],
            rpc.get_latest_blockhash().await.unwrap(),
        ))
        .await
        .unwrap()
        .value;
    let expected = CommitmentError::VestingFloorBreached.code();
    match refused.err {
        Some(TransactionError::InstructionError(_, InstructionError::Custom(code)))
            if code == expected =>
        {
            println!(
                "PASS  transfer of 1 below the floor refused by the hook (custom error {code:#x})"
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
        "config PDA         {}",
        config_address(&mint.pubkey(), &args.program_id).0
    );
    println!(
        "validation list    {}",
        hook_kit::validation_list_address(&mint.pubkey(), &args.program_id).0
    );
    println!("creator account    {}", creator.pubkey());
    println!("recipient          {}", recipient.pubkey());
    println!(
        "\nnext: the mint's Transfer Hook authority ({}) can still re-point the mint and remove the \
         floor, and the program's upgrade authority can replace the rule. Revoke both before calling \
         a commitment durable.",
        payer.pubkey()
    );
}

async fn rent(rpc: &RpcClient, len: usize) -> u64 {
    rpc.get_minimum_balance_for_rent_exemption(len)
        .await
        .unwrap_or_else(|error| fail(&format!("cannot read rent: {error}")))
}

async fn transfer(
    rpc: &RpcClient,
    payer: &Keypair,
    source: &Keypair,
    destination: &Keypair,
    mint: &Keypair,
    amount: u64,
) -> Instruction {
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

async fn send(rpc: &RpcClient, payer: &Keypair, instructions: &[Instruction], extra: &[&Keypair]) {
    let mut signers: Vec<&Keypair> = vec![payer];
    signers.extend_from_slice(extra);
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
