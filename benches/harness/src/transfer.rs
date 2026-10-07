//! The raw Token-2022 transfer scenario: a transfer through the bench hook with `N` extra accounts,
//! measured as a legacy transaction and as a v0 transaction with an address lookup table.

use hook_kit::testing::{AccountSpec, World};
use solana_program_test::{processor, ProgramTest};
use std::borrow::Cow;

use solana_sdk::{
    account::{Account, AccountSharedData},
    address_lookup_table::{
        state::{AddressLookupTable, LookupTableMeta},
        AddressLookupTableAccount,
    },
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    transaction::{Transaction, VersionedTransaction},
};
use spl_tlv_account_resolution::state::ExtraAccountMetaList;

use crate::report::{FormatResult, TransferRow, HEAP_FRAME_BYTES, PACKET_DATA_SIZE};

/// The compute-unit limit every benchmark transaction requests.
pub const COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// A program id for the bench hook in-process (any id will do).
pub fn bench_program_id() -> Pubkey {
    Pubkey::new_from_array([0xBE; 32])
}

fn clone(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("keypair bytes")
}

/// Serialised size of a transaction with `signatures` signatures and a message of `message_len`.
fn transaction_size(signatures: usize, message_len: usize) -> usize {
    // compact-u16 signature count (one byte below 128), the signatures, then the message.
    1 + signatures * 64 + message_len
}

/// Run a simulation and pull out the compute units, or the reason it failed.
async fn simulate(
    world: &mut World,
    tx: impl Into<VersionedTransaction>,
) -> (Option<u64>, Option<String>) {
    match world.context.banks_client.simulate_transaction(tx).await {
        Ok(outcome) => {
            let failed = matches!(outcome.result, Some(Err(_)));
            let (units, tail) = match outcome.simulation_details {
                Some(d) => {
                    let skip = d.logs.len().saturating_sub(12);
                    (Some(d.units_consumed), d.logs[skip..].join(" | "))
                }
                None => (None, String::new()),
            };
            let error = match outcome.result {
                Some(Err(e)) if failed => Some(format!("the simulation failed: {e} [{tail}]")),
                _ => None,
            };
            (units, error)
        }
        Err(e) => (None, Some(format!("the simulation could not run: {e}"))),
    }
}

fn writable_count(message: &solana_sdk::message::Message) -> usize {
    (0..message.account_keys.len())
        .filter(|i| message.is_maybe_writable(*i, None))
        .count()
}

async fn legacy(
    world: &mut World,
    transfer: &Instruction,
    payer: &Keypair,
    heap_frame: Option<u32>,
) -> FormatResult {
    let blockhash = world
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .expect("a blockhash");
    let mut instructions = vec![ComputeBudgetInstruction::set_compute_unit_limit(
        COMPUTE_UNIT_LIMIT,
    )];
    if let Some(bytes) = heap_frame {
        instructions.push(ComputeBudgetInstruction::request_heap_frame(bytes));
    }
    instructions.push(transfer.clone());
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        blockhash,
    );
    let bytes = transaction_size(tx.signatures.len(), tx.message.serialize().len());
    let accounts = tx.message.account_keys.len();
    let writable = writable_count(&tx.message);
    let (compute_units, error) = simulate(world, tx).await;
    FormatResult {
        fits_a_packet: bytes <= PACKET_DATA_SIZE,
        bytes: Some(bytes),
        accounts: Some(accounts),
        writable_accounts: Some(writable),
        compute_units,
        error: error.or_else(|| {
            (bytes > PACKET_DATA_SIZE).then(|| {
                format!("{bytes} bytes is over the {PACKET_DATA_SIZE}-byte packet: a cluster would reject it")
            })
        }),
    }
}

/// Build the lookup table holding every non-signer account of `transfer` and return it.
///
/// `solana-program-test` does not run the address-lookup-table program (it is a Core BPF
/// program on a cluster), so the table cannot be created with its instructions here. The runtime
/// only needs the table as an account owned by that program, so one is installed directly,
/// serialised exactly as the program would. What is measured is therefore the transaction that
/// *uses* a table, not the cost of creating or extending one.
async fn lookup_table(
    world: &mut World,
    transfer: &Instruction,
    payer: &Keypair,
) -> Result<AddressLookupTableAccount, String> {
    let addresses: Vec<Pubkey> = transfer
        .accounts
        .iter()
        .filter(|meta| !meta.is_signer)
        .map(|meta| meta.pubkey)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let state = AddressLookupTable {
        meta: LookupTableMeta::new(payer.pubkey()),
        addresses: Cow::Owned(addresses.clone()),
    };
    let data = state
        .serialize_for_tests()
        .map_err(|e| format!("serialising the lookup table failed: {e}"))?;
    let key = Pubkey::new_unique();
    let rent = Rent::default().minimum_balance(data.len());
    let account = Account {
        lamports: rent,
        data,
        owner: solana_sdk::address_lookup_table::program::id(),
        executable: false,
        rent_epoch: 0,
    };
    // The bank checks that the lamports in existence do not change, so the table's rent is paid
    // by the payer.
    let mut funder = world
        .context
        .banks_client
        .get_account(payer.pubkey())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("the payer account is missing")?;
    funder.lamports = funder
        .lamports
        .checked_sub(rent)
        .ok_or("the payer cannot fund the lookup table")?;
    world
        .context
        .set_account(&payer.pubkey(), &AccountSharedData::from(funder));
    world
        .context
        .set_account(&key, &AccountSharedData::from(account));
    // Addresses are usable from the slot after the table was last extended (slot 0 here).
    let slot = world
        .context
        .banks_client
        .get_sysvar::<solana_sdk::clock::Clock>()
        .await
        .map_err(|e| e.to_string())?
        .slot;
    world
        .context
        .warp_to_slot(slot + 2)
        .map_err(|e| format!("{e:?}"))?;
    Ok(AddressLookupTableAccount { key, addresses })
}

async fn v0_with_lookup_table(
    world: &mut World,
    transfer: &Instruction,
    payer: &Keypair,
) -> (FormatResult, Option<usize>) {
    let table = match lookup_table(world, transfer, payer).await {
        Ok(table) => table,
        Err(error) => {
            return (
                FormatResult {
                    error: Some(error),
                    ..Default::default()
                },
                None,
            )
        }
    };
    let entries = table.addresses.len();
    let blockhash = world
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .expect("a blockhash");
    let instructions = [
        ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT),
        transfer.clone(),
    ];
    let message =
        match v0::Message::try_compile(&payer.pubkey(), &instructions, &[table], blockhash) {
            Ok(message) => message,
            Err(e) => {
                return (
                    FormatResult {
                        error: Some(format!("compiling the v0 message failed: {e}")),
                        ..Default::default()
                    },
                    Some(entries),
                )
            }
        };
    let loaded: usize = message
        .address_table_lookups
        .iter()
        .map(|l| l.writable_indexes.len() + l.readonly_indexes.len())
        .sum();
    let loaded_writable: usize = message
        .address_table_lookups
        .iter()
        .map(|l| l.writable_indexes.len())
        .sum();
    let accounts = message.account_keys.len() + loaded;
    let static_writable = (0..message.account_keys.len())
        .filter(|i| message.is_maybe_writable(*i, None))
        .count();
    let versioned = VersionedMessage::V0(message);
    let tx = match VersionedTransaction::try_new(versioned.clone(), &[payer]) {
        Ok(tx) => tx,
        Err(e) => {
            return (
                FormatResult {
                    error: Some(format!("signing the v0 transaction failed: {e}")),
                    ..Default::default()
                },
                Some(entries),
            )
        }
    };
    let bytes = transaction_size(tx.signatures.len(), versioned.serialize().len());
    let (compute_units, error) = simulate(world, tx).await;
    (
        FormatResult {
            fits_a_packet: bytes <= PACKET_DATA_SIZE,
            bytes: Some(bytes),
            accounts: Some(accounts),
            writable_accounts: Some(static_writable + loaded_writable),
            compute_units,
            error: error.or_else(|| {
                (bytes > PACKET_DATA_SIZE).then(|| {
                    format!("{bytes} bytes is over the {PACKET_DATA_SIZE}-byte packet: a cluster would reject it")
                })
            }),
        },
        Some(entries),
    )
}

/// Measure one transfer through the bench hook with `extras` extra accounts.
pub async fn measure(extras: u8, writable_counter: bool) -> TransferRow {
    let list_bytes = ExtraAccountMetaList::size_of(extras as usize).unwrap_or(0);
    let mut row = TransferRow {
        extras,
        writable_counter,
        hook_accounts: extras as usize + 2,
        validation_list_bytes: list_bytes,
        validation_list_rent_lamports: Rent::default().minimum_balance(list_bytes),
        ..Default::default()
    };
    let id = bench_program_id();
    let test = ProgramTest::new(
        "bench_hook",
        id,
        processor!(bench_hook::process_instruction),
    );
    let mut world = World::start(
        test,
        id,
        vec![AccountSpec::payer_owned(1_000), AccountSpec::payer_owned(0)],
    )
    .await;
    let payer = clone(&world.context.payer);
    let init = bench_hook::initialize_instruction(
        &id,
        &payer.pubkey(),
        &payer.pubkey(),
        &world.mint.pubkey(),
        extras,
        writable_counter,
    );
    if let Err(e) = world.send(&[init], &[]).await {
        let error = Some(format!("initialising the hook failed: {e}"));
        row.legacy.error = error.clone();
        row.v0_lookup_table.error = error;
        return row;
    }
    let allow: Vec<Pubkey> = if writable_counter && extras > 0 {
        vec![bench_hook::counter_address(&id, &world.mint.pubkey())]
    } else {
        Vec::new()
    };
    let transfer = world.transfer_ix(0, 1, 10, &allow).await;
    row.legacy = legacy(&mut world, &transfer, &payer, None).await;
    if row
        .legacy
        .error
        .as_deref()
        .is_some_and(|e| e.contains("out of memory"))
    {
        row.legacy_with_heap_frame =
            Some(legacy(&mut world, &transfer, &payer, Some(HEAP_FRAME_BYTES)).await);
    }
    let (v0, entries) = v0_with_lookup_table(&mut world, &transfer, &payer).await;
    row.v0_lookup_table = v0;
    row.lookup_table_entries = entries;
    row
}
