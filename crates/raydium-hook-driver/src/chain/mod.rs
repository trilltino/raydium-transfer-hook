//! The chain abstraction. A flow talks to a [`Chain`], so the identical code runs against a real
//! RPC endpoint (devnet) and against `solana-program-test` (the same runtime, in-process).

use std::{fmt, future::Future, pin::Pin, sync::Arc};

use solana_sdk::{
    account::Account,
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::{Transaction, TransactionError},
};

/// A failure from the chain, with the program's custom error code when the runtime reported one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverError {
    pub message: String,
    pub custom_code: Option<u32>,
}

impl DriverError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            custom_code: None,
        }
    }

    pub fn from_transaction_error(error: &TransactionError) -> Self {
        let custom_code = match error {
            TransactionError::InstructionError(_, InstructionError::Custom(code)) => Some(*code),
            _ => None,
        };
        Self {
            message: error.to_string(),
            custom_code,
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.custom_code {
            Some(code) => write!(f, "{} (custom program error {code:#x})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for DriverError {}

pub type Result<T> = std::result::Result<T, DriverError>;

/// Reads accounts. Cloneable so a resolver can fetch lazily while the chain stays borrowed.
pub type Reader = Arc<
    dyn Fn(Pubkey) -> Pin<Box<dyn Future<Output = Result<Option<Account>>> + Send>> + Send + Sync,
>;

#[derive(Clone, Debug)]
pub struct Sent {
    pub signature: String,
}

#[derive(Clone, Debug)]
pub struct Simulation {
    pub succeeded: bool,
    pub error: Option<DriverError>,
    pub logs: Vec<String>,
    pub units_consumed: Option<u64>,
}

impl Simulation {
    /// How many times `program` was invoked (at any depth).
    pub fn invocations_of(&self, program: &Pubkey) -> usize {
        let prefix = format!("Program {program} invoke");
        self.logs.iter().filter(|l| l.starts_with(&prefix)).count()
    }

    /// Whether `program` reported `custom program error: {code}`.
    pub fn program_failed_with(&self, program: &Pubkey, code: u32) -> bool {
        let line = format!("Program {program} failed: custom program error: {code:#x}");
        self.logs.iter().any(|l| l == &line)
    }
}

#[allow(async_fn_in_trait)]
pub trait Chain {
    /// The fee payer and default owner of everything the flows create.
    fn payer(&self) -> &Keypair;
    fn reader(&self) -> Reader;
    /// Sign with the payer plus `signers`, send and confirm.
    async fn send(&mut self, instructions: &[Instruction], signers: &[&Keypair]) -> Result<Sent>;
    /// Simulate (no state change) with the same signers.
    async fn simulate(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<Simulation>;

    async fn account(&mut self, key: &Pubkey) -> Result<Option<Account>> {
        (self.reader())(*key).await
    }

    /// Let at least `seconds` of cluster time pass (a pool is only tradable after its open time).
    async fn advance_time(&mut self, seconds: u64) -> Result<()>;

    /// Whether `advance_time` moves the clock at once (an in-process bank) rather than waiting for it.
    /// A flow with something that takes days only runs it where this is true.
    fn can_warp(&self) -> bool {
        false
    }
}

pub(super) fn sign(
    payer: &Keypair,
    instructions: &[Instruction],
    signers: &[&Keypair],
    blockhash: solana_sdk::hash::Hash,
) -> Transaction {
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend_from_slice(signers);
    Transaction::new_signed_with_payer(instructions, Some(&payer.pubkey()), &all, blockhash)
}

mod rpc;
pub use rpc::RpcChain;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::LocalChain;
