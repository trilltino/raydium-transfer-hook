//! Approving hooked mints for pool creation.
//!
//! CPMM and CLMM admit a Token-2022 mint with a TransferHook extension to a new pool only if a
//! `SupportMintAssociated` record exists for it. The programs accept exactly two signers for
//! `create_support_mint_associated`: their compile-time admin and one fixed owner key. So whoever
//! deploys the forks with their own keys (a fork, a hackathon operator) approves the mints; nobody
//! else can, and nothing here pretends otherwise.
//!
//! This module is the logic behind `raydium-hook mint approve` and `mint approval`, and the flows
//! use it for the same step, so the command is exercised by every end-to-end run. It is generic
//! over [`Chain`], so the tests run it in-process against the real Raydium binaries.

use std::fmt;

use solana_sdk::{
    hash::hash, instruction::Instruction, message::Message, pubkey::Pubkey, signature::Signer,
};

use crate::{
    chain::{Chain, DriverError, Result},
    clmm::Clmm,
    cpmm::Cpmm,
    env::Environment,
    inspect_readiness, Readiness,
};

/// Largest transaction a cluster accepts.
const PACKET_DATA_SIZE: usize = 1232;

/// Which Raydium program an approval is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Amm {
    Cpmm,
    Clmm,
}

impl Amm {
    pub fn name(self) -> &'static str {
        match self {
            Amm::Cpmm => "cpmm",
            Amm::Clmm => "clmm",
        }
    }

    /// The program's error code when a pool is created with an unapproved hooked mint
    /// (`NotSupportMint`; Anchor numbers errors from 6000).
    pub fn not_support_mint_code(self) -> u32 {
        match self {
            Amm::Cpmm => 6007,
            Amm::Clmm => 6034,
        }
    }

    /// Parse `cpmm`, `clmm` or `all`.
    pub fn parse_list(text: &str) -> Result<Vec<Amm>> {
        match text {
            "cpmm" => Ok(vec![Amm::Cpmm]),
            "clmm" => Ok(vec![Amm::Clmm]),
            "all" => Ok(vec![Amm::Cpmm, Amm::Clmm]),
            other => Err(DriverError::new(format!(
                "unknown AMM `{other}`: cpmm, clmm or all"
            ))),
        }
    }
}

/// The program id and the approval record address of `mint` on `amm`.
pub fn record_address(env: &Environment, amm: Amm, mint: &Pubkey) -> Result<(Pubkey, Pubkey)> {
    Ok(match amm {
        Amm::Cpmm => {
            let program = env.cpmm_program()?;
            let cpmm = Cpmm {
                program_id: program,
                fee_receiver: Pubkey::default(),
            };
            (program, cpmm.support_mint(mint))
        }
        Amm::Clmm => {
            let program = env.clmm_program()?;
            let clmm = Clmm {
                program_id: program,
            };
            (program, clmm.support_mint(mint))
        }
    })
}

/// What the approval record of one mint looks like on one AMM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordState {
    Approved,
    NotApproved,
    /// The account exists but is not a record for this mint.
    Invalid(String),
}

fn account_discriminator() -> [u8; 8] {
    hash(b"account:SupportMintAssociated").to_bytes()[..8]
        .try_into()
        .expect("eight bytes")
}

/// Judge the account found at a record address. The program owns the address, so a correct owner,
/// discriminator and stored mint is what `create_support_mint_associated` leaves behind.
pub fn judge_record(
    account: Option<&solana_sdk::account::Account>,
    program: &Pubkey,
    mint: &Pubkey,
) -> RecordState {
    let Some(account) = account else {
        return RecordState::NotApproved;
    };
    if account.executable || &account.owner != program {
        return RecordState::Invalid(format!("owned by {}, not by the program", account.owner));
    }
    // 8-byte discriminator, 1-byte bump, 32-byte mint, then padding.
    if account.data.len() < 8 + 1 + 32 || account.data[..8] != account_discriminator() {
        return RecordState::Invalid("not a SupportMintAssociated account".into());
    }
    if account.data[9..41] != mint.to_bytes() {
        return RecordState::Invalid("the record names a different mint".into());
    }
    RecordState::Approved
}

/// Read the approval state of `mint` on `amm`.
pub async fn state<C: Chain>(
    chain: &mut C,
    env: &Environment,
    amm: Amm,
    mint: &Pubkey,
) -> Result<RecordState> {
    let (program, record) = record_address(env, amm, mint)?;
    let account = chain.account(&record).await?;
    Ok(judge_record(account.as_ref(), &program, mint))
}

/// Whether a mint can and needs to be approved, from its transport facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MintCheck {
    /// Ready for approval. The string is a note for the operator, empty if there is none.
    Approvable(String),
    /// Nothing to approve: no TransferHook extension, so no record is needed.
    NotNeeded(String),
    /// Approving would be a mistake.
    Blocked(String),
}

/// Judge a mint's readiness. A mint whose hook is not set yet is approvable, because the usual
/// order is to approve the mint, create the pool, then attach the hook.
pub fn check_mint(readiness: &Readiness) -> MintCheck {
    if !readiness.mint_exists {
        return MintCheck::Blocked("the mint account does not exist".into());
    }
    if !readiness.token_2022 {
        return MintCheck::Blocked("not a Token-2022 mint".into());
    }
    if !readiness.has_hook_extension {
        return MintCheck::NotNeeded(
            "the mint has no TransferHook extension, so no approval is needed".into(),
        );
    }
    let Some(program) = &readiness.hook_program else {
        return MintCheck::Approvable(
            "the hook is not set yet; attach it after the pool exists".into(),
        );
    };
    match &readiness.program {
        Some(facts) if !facts.exists => {
            return MintCheck::Blocked(format!("hook program {program} does not exist"))
        }
        Some(facts) if !facts.executable => {
            return MintCheck::Blocked(format!("hook program {program} is not executable"))
        }
        _ => {}
    }
    let problems = readiness.transport_problems();
    if problems.is_empty() {
        MintCheck::Approvable(String::new())
    } else {
        // A broken validation list does not make approval wrong (the hook may be initialised
        // after), but the operator should know.
        MintCheck::Approvable(problems.join("; "))
    }
}

/// What happened to one mint on one AMM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    AlreadyApproved,
    /// Sent and confirmed.
    Approved {
        signature: String,
    },
    /// Simulated successfully; `--dry-run` sent nothing.
    WouldApprove,
    NotNeeded(String),
    Blocked(String),
    Failed(String),
}

impl Outcome {
    /// Whether the operator needs to act: a mint that was blocked or failed.
    pub fn is_problem(&self) -> bool {
        matches!(self, Outcome::Blocked(_) | Outcome::Failed(_))
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::AlreadyApproved => write!(f, "already approved"),
            Outcome::Approved { signature } => write!(f, "approved  {signature}"),
            Outcome::WouldApprove => write!(f, "would approve (dry run, simulation passed)"),
            Outcome::NotNeeded(why) => write!(f, "not needed: {why}"),
            Outcome::Blocked(why) => write!(f, "NOT approved: {why}"),
            Outcome::Failed(why) => write!(f, "FAILED: {why}"),
        }
    }
}

/// One line of the result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub amm: Amm,
    pub mint: Pubkey,
    pub outcome: Outcome,
    /// Something to tell the operator even when the mint was approved.
    pub note: String,
}

/// Remove anything that looks like a URL (an RPC URL can carry an API key) from an error message.
pub fn redact(message: &str) -> String {
    message
        .split_whitespace()
        .map(|word| {
            if word.contains("://") {
                "[URL redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The serialised size of a transaction with these instructions and one signer.
fn transaction_size(instructions: &[Instruction], payer: &Pubkey) -> usize {
    let message = Message::new(instructions, Some(payer));
    1 + 64 * usize::from(message.header.num_required_signatures) + message.serialize().len()
}

/// Group instructions, in order, into the fewest transactions that each fit a packet.
pub fn pack(instructions: Vec<Instruction>, payer: &Pubkey) -> Result<Vec<Vec<Instruction>>> {
    let mut batches: Vec<Vec<Instruction>> = Vec::new();
    let mut current: Vec<Instruction> = Vec::new();
    for instruction in instructions {
        let mut candidate = current.clone();
        candidate.push(instruction.clone());
        if transaction_size(&candidate, payer) <= PACKET_DATA_SIZE {
            current = candidate;
            continue;
        }
        if current.is_empty() {
            return Err(DriverError::new(
                "one approval instruction does not fit in a transaction",
            ));
        }
        batches.push(std::mem::take(&mut current));
        if transaction_size(std::slice::from_ref(&instruction), payer) > PACKET_DATA_SIZE {
            return Err(DriverError::new(
                "one approval instruction does not fit in a transaction",
            ));
        }
        current.push(instruction);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

/// Approve `mints` on `amms`. The chain's payer must be the program admin the environment records.
///
/// Per mint: check it is worth approving, skip it if it already is, then approve the rest of each
/// AMM's mints in as few transactions as fit, simulating each first. With `dry_run` nothing is
/// sent. A mint that is blocked or fails never stops the others.
pub async fn approve<C: Chain>(
    chain: &mut C,
    env: &Environment,
    amms: &[Amm],
    mints: &[Pubkey],
    dry_run: bool,
) -> Result<Vec<Row>> {
    let payer = chain.payer().pubkey();
    let admin = env.admin_key()?;
    if payer != admin {
        return Err(DriverError::new(format!(
            "this key ({payer}) cannot approve mints: the program admin recorded in the environment is {admin}. \
             Only the admin (or the one fixed owner key built into the program) can sign the approval."
        )));
    }

    let mut rows: Vec<Row> = Vec::new();
    // Readiness does not depend on the AMM, so read each mint once.
    let mut checks: Vec<(Pubkey, MintCheck)> = Vec::new();
    for mint in mints {
        if checks.iter().any(|(m, _)| m == mint) {
            continue;
        }
        let readiness = inspect_readiness(&chain.reader(), *mint)
            .await
            .map_err(|e| DriverError::new(redact(&e.to_string())))?;
        checks.push((*mint, check_mint(&readiness)));
    }

    for amm in amms {
        let mut pending: Vec<(Pubkey, String)> = Vec::new();
        for (mint, check) in &checks {
            match check {
                MintCheck::Blocked(why) => rows.push(Row {
                    amm: *amm,
                    mint: *mint,
                    outcome: Outcome::Blocked(why.clone()),
                    note: String::new(),
                }),
                MintCheck::NotNeeded(why) => rows.push(Row {
                    amm: *amm,
                    mint: *mint,
                    outcome: Outcome::NotNeeded(why.clone()),
                    note: String::new(),
                }),
                MintCheck::Approvable(note) => match state(chain, env, *amm, mint).await? {
                    RecordState::Approved => rows.push(Row {
                        amm: *amm,
                        mint: *mint,
                        outcome: Outcome::AlreadyApproved,
                        note: note.clone(),
                    }),
                    RecordState::Invalid(why) => rows.push(Row {
                        amm: *amm,
                        mint: *mint,
                        outcome: Outcome::Blocked(format!(
                            "an account already sits at the record address but is not a valid record: {why}"
                        )),
                        note: String::new(),
                    }),
                    RecordState::NotApproved => pending.push((*mint, note.clone())),
                },
            }
        }
        if pending.is_empty() {
            continue;
        }

        let instructions: Vec<Instruction> = pending
            .iter()
            .map(|(mint, _)| match amm {
                Amm::Cpmm => Cpmm {
                    program_id: env.cpmm_program().expect("checked by record_address"),
                    fee_receiver: Pubkey::default(),
                }
                .create_support_mint_instruction(&admin, mint),
                Amm::Clmm => Clmm {
                    program_id: env.clmm_program().expect("checked by record_address"),
                }
                .create_support_mint_instruction(&admin, mint),
            })
            .collect();

        // Packing keeps instruction order, so batch `n` covers a known slice of `pending`.
        let batches = pack(instructions, &payer)?;
        let mut offset = 0;
        for batch in batches {
            let covered = &pending[offset..offset + batch.len()];
            offset += batch.len();
            let simulation = chain.simulate(&batch, &[]).await;
            let failure = match simulation {
                Ok(sim) if sim.succeeded => None,
                Ok(sim) => Some(format!(
                    "the simulation failed: {}",
                    sim.error
                        .map(|e| redact(&e.to_string()))
                        .unwrap_or_else(|| "unknown error".into())
                )),
                Err(e) => Some(format!(
                    "the simulation could not run: {}",
                    redact(&e.to_string())
                )),
            };
            let outcome = match failure {
                Some(reason) => Outcome::Failed(reason),
                None if dry_run => Outcome::WouldApprove,
                None => match chain.send(&batch, &[]).await {
                    Ok(sent) => Outcome::Approved {
                        signature: sent.signature,
                    },
                    Err(e) => Outcome::Failed(redact(&e.to_string())),
                },
            };
            for (mint, note) in covered {
                rows.push(Row {
                    amm: *amm,
                    mint: *mint,
                    outcome: outcome.clone(),
                    note: note.clone(),
                });
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{account::Account, instruction::AccountMeta};

    fn record(program: Pubkey, mint: Pubkey) -> Account {
        let mut data = account_discriminator().to_vec();
        data.push(255);
        data.extend_from_slice(&mint.to_bytes());
        data.extend_from_slice(&[0; 64]);
        Account {
            lamports: 1,
            data,
            owner: program,
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn the_discriminator_is_anchors_account_hash() {
        // sha256("account:SupportMintAssociated")[..8], the bytes the program writes first.
        assert_eq!(account_discriminator().len(), 8);
        assert_eq!(
            account_discriminator(),
            hash(b"account:SupportMintAssociated").to_bytes()[..8]
        );
    }

    #[test]
    fn a_record_is_approved_only_if_owner_discriminator_and_mint_all_match() {
        let (program, mint) = (Pubkey::new_unique(), Pubkey::new_unique());
        assert_eq!(
            judge_record(None, &program, &mint),
            RecordState::NotApproved
        );
        assert_eq!(
            judge_record(Some(&record(program, mint)), &program, &mint),
            RecordState::Approved
        );
        // Owned by someone else: an attacker cannot approve by funding the address.
        let foreign = record(Pubkey::new_unique(), mint);
        assert!(matches!(
            judge_record(Some(&foreign), &program, &mint),
            RecordState::Invalid(_)
        ));
        // Right owner, wrong mint inside.
        let other = record(program, Pubkey::new_unique());
        assert!(matches!(
            judge_record(Some(&other), &program, &mint),
            RecordState::Invalid(_)
        ));
        // Right owner, not a record at all.
        let mut junk = record(program, mint);
        junk.data[..8].copy_from_slice(&[1; 8]);
        assert!(matches!(
            judge_record(Some(&junk), &program, &mint),
            RecordState::Invalid(_)
        ));
        junk.data.truncate(10);
        assert!(matches!(
            judge_record(Some(&junk), &program, &mint),
            RecordState::Invalid(_)
        ));
    }

    #[test]
    fn urls_are_removed_from_messages() {
        let message =
            "error sending request for url (https://rpc.example/?api-key=SECRET): timed out";
        let cleaned = redact(message);
        assert!(!cleaned.contains("SECRET"));
        assert!(cleaned.contains("[URL redacted]"));
        assert_eq!(redact("plain message"), "plain message");
    }

    #[test]
    fn many_approvals_pack_into_transactions_that_each_fit_a_packet() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let instructions: Vec<Instruction> = (0..40)
            .map(|_| Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(payer, true),
                    AccountMeta::new_readonly(Pubkey::new_unique(), false),
                    AccountMeta::new(Pubkey::new_unique(), false),
                    AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
                ],
                data: vec![1; 8],
            })
            .collect();
        let batches = pack(instructions.clone(), &payer).unwrap();
        assert!(batches.len() > 1, "40 approvals cannot share one packet");
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 40);
        for batch in &batches {
            assert!(transaction_size(batch, &payer) <= PACKET_DATA_SIZE);
        }
        // Order is preserved.
        let flat: Vec<Instruction> = batches.into_iter().flatten().collect();
        assert_eq!(flat, instructions);
    }

    #[test]
    fn mints_are_judged_from_their_transport_facts() {
        let mint = Pubkey::new_unique();
        let base = Readiness {
            mint,
            mint_exists: true,
            token_2022: true,
            has_hook_extension: true,
            hook_program: None,
            hook_authority: None,
            program: None,
            validation_list: None,
        };
        assert!(matches!(
            check_mint(&Readiness {
                mint_exists: false,
                ..base.clone()
            }),
            MintCheck::Blocked(_)
        ));
        assert!(matches!(
            check_mint(&Readiness {
                token_2022: false,
                ..base.clone()
            }),
            MintCheck::Blocked(_)
        ));
        assert!(matches!(
            check_mint(&Readiness {
                has_hook_extension: false,
                ..base.clone()
            }),
            MintCheck::NotNeeded(_)
        ));
        // The usual order: the hook is attached after the pool exists, so an unset hook is fine.
        assert!(matches!(check_mint(&base), MintCheck::Approvable(_)));
    }

    #[test]
    fn only_the_listed_amms_parse() {
        assert_eq!(Amm::parse_list("all").unwrap(), vec![Amm::Cpmm, Amm::Clmm]);
        assert_eq!(Amm::parse_list("cpmm").unwrap(), vec![Amm::Cpmm]);
        assert!(Amm::parse_list("orca").is_err());
        assert_ne!(
            Amm::Cpmm.not_support_mint_code(),
            Amm::Clmm.not_support_mint_code()
        );
    }
}
