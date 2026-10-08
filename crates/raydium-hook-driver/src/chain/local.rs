//! The Solana runtime in-process via `solana-program-test`.

use std::sync::Arc;

use solana_program_test::{BanksClientError, ProgramTestContext};
use solana_sdk::{instruction::Instruction, signature::Keypair};

use super::{sign, Chain, DriverError, Reader, Result, Sent, Simulation};

/// The Solana runtime in-process via `solana-program-test`. Same program binaries, same
/// execution engine as a validator, but no RPC server and no network.
pub struct LocalChain<'a> {
    pub context: &'a mut ProgramTestContext,
    payer: Keypair,
    /// The size in bytes of the largest transaction sent so far (a packet holds 1232).
    pub largest_transaction: usize,
}

impl<'a> LocalChain<'a> {
    /// Sign as the context's own payer.
    pub fn new(context: &'a mut ProgramTestContext) -> Self {
        let payer = Keypair::from_bytes(&context.payer.to_bytes()).expect("payer keypair");
        Self::with_payer(context, payer)
    }

    /// Sign as `payer` (it must hold lamports in the test bank). Needed to act as the
    /// integration build's admin.
    pub fn with_payer(context: &'a mut ProgramTestContext, payer: Keypair) -> Self {
        Self {
            context,
            payer,
            largest_transaction: 0,
        }
    }
}

/// `ProgramTest` does not enforce the packet limit a real cluster does, so enforce it here: a flow
/// that only fits locally would fail on devnet. Returns the transaction's size.
fn ensure_fits_in_a_packet(tx: &solana_sdk::transaction::Transaction) -> Result<usize> {
    // compact-u16 signature count (one byte below 128), the signatures, then the message.
    let size = 1 + tx.signatures.len() * 64 + tx.message.serialize().len();
    if size > solana_sdk::packet::PACKET_DATA_SIZE {
        return Err(DriverError::new(format!(
            "the transaction is {size} bytes, over the {} byte packet limit",
            solana_sdk::packet::PACKET_DATA_SIZE
        )));
    }
    Ok(size)
}

fn banks_error(error: BanksClientError) -> DriverError {
    match error {
        BanksClientError::TransactionError(e) => DriverError::from_transaction_error(&e),
        BanksClientError::SimulationError { err, logs, .. } => {
            let mut driver = DriverError::from_transaction_error(&err);
            driver.message = format!("{} [{}]", driver.message, logs.join(" | "));
            driver
        }
        other => DriverError::new(other.to_string()),
    }
}

impl Chain for LocalChain<'_> {
    fn payer(&self) -> &Keypair {
        &self.payer
    }

    fn reader(&self) -> Reader {
        let client = self.context.banks_client.clone();
        Arc::new(move |key| {
            let client = client.clone();
            Box::pin(async move { client.get_account(key).await.map_err(banks_error) })
        })
    }

    async fn send(&mut self, instructions: &[Instruction], signers: &[&Keypair]) -> Result<Sent> {
        let blockhash = self
            .context
            .banks_client
            .get_latest_blockhash()
            .await
            .map_err(banks_error)?;
        let tx = sign(&self.payer, instructions, signers, blockhash);
        let size = ensure_fits_in_a_packet(&tx)?;
        self.largest_transaction = self.largest_transaction.max(size);
        let signature = tx.signatures[0].to_string();
        self.context
            .banks_client
            .process_transaction(tx)
            .await
            .map_err(banks_error)?;
        Ok(Sent { signature })
    }

    fn can_warp(&self) -> bool {
        true
    }

    async fn advance_time(&mut self, seconds: u64) -> Result<()> {
        let mut clock: solana_sdk::clock::Clock = self
            .context
            .banks_client
            .get_sysvar()
            .await
            .map_err(banks_error)?;
        clock.unix_timestamp += i64::try_from(seconds + 1).unwrap_or(i64::MAX);
        self.context.set_sysvar(&clock);
        Ok(())
    }

    async fn simulate(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<Simulation> {
        let blockhash = self
            .context
            .banks_client
            .get_latest_blockhash()
            .await
            .map_err(banks_error)?;
        let tx = sign(&self.payer, instructions, signers, blockhash);
        ensure_fits_in_a_packet(&tx)?;
        let outcome = self
            .context
            .banks_client
            .simulate_transaction(tx)
            .await
            .map_err(banks_error)?;
        let details = outcome.simulation_details;
        let result = outcome.result;
        Ok(Simulation {
            succeeded: matches!(result, Some(Ok(()))),
            error: match result {
                Some(Err(e)) => Some(DriverError::from_transaction_error(&e)),
                _ => None,
            },
            logs: details.as_ref().map(|d| d.logs.clone()).unwrap_or_default(),
            units_consumed: details.map(|d| d.units_consumed),
        })
    }
}
