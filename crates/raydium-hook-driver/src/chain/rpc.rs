//! A real cluster behind a JSON-RPC endpoint.

use std::sync::Arc;

use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, signature::Keypair,
};

use super::{sign, Chain, DriverError, Reader, Result, Sent, Simulation};

/// A real cluster behind a JSON-RPC endpoint.
pub struct RpcChain {
    client: Arc<RpcClient>,
    payer: Keypair,
    commitment: CommitmentConfig,
}

impl RpcChain {
    pub fn new(rpc_url: impl Into<String>, payer: Keypair) -> Self {
        let commitment = CommitmentConfig::confirmed();
        Self {
            client: Arc::new(RpcClient::new_with_commitment(rpc_url.into(), commitment)),
            payer,
            commitment,
        }
    }

    pub fn client(&self) -> &RpcClient {
        &self.client
    }
}

impl Chain for RpcChain {
    fn payer(&self) -> &Keypair {
        &self.payer
    }

    fn reader(&self) -> Reader {
        let client = self.client.clone();
        let commitment = self.commitment;
        Arc::new(move |key| {
            let client = client.clone();
            Box::pin(async move {
                client
                    .get_account_with_commitment(&key, commitment)
                    .await
                    .map(|response| response.value)
                    .map_err(|e| DriverError::new(format!("get_account {key}: {e}")))
            })
        })
    }

    async fn send(&mut self, instructions: &[Instruction], signers: &[&Keypair]) -> Result<Sent> {
        let blockhash = self
            .client
            .get_latest_blockhash()
            .await
            .map_err(|e| DriverError::new(format!("get_latest_blockhash: {e}")))?;
        let tx = sign(&self.payer, instructions, signers, blockhash);
        match self.client.send_and_confirm_transaction(&tx).await {
            Ok(signature) => Ok(Sent {
                signature: signature.to_string(),
            }),
            Err(error) => Err(match error.get_transaction_error() {
                Some(tx_error) => DriverError::from_transaction_error(&tx_error),
                None => DriverError::new(error.to_string()),
            }),
        }
    }

    async fn advance_time(&mut self, seconds: u64) -> Result<()> {
        tokio::time::sleep(std::time::Duration::from_secs(seconds + 1)).await;
        Ok(())
    }

    async fn simulate(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<Simulation> {
        let blockhash = self
            .client
            .get_latest_blockhash()
            .await
            .map_err(|e| DriverError::new(format!("get_latest_blockhash: {e}")))?;
        let tx = sign(&self.payer, instructions, signers, blockhash);
        let response = self
            .client
            .simulate_transaction(&tx)
            .await
            .map_err(|e| DriverError::new(format!("simulate_transaction: {e}")))?;
        let value = response.value;
        Ok(Simulation {
            succeeded: value.err.is_none(),
            error: value.err.as_ref().map(DriverError::from_transaction_error),
            logs: value.logs.unwrap_or_default(),
            units_consumed: value.units_consumed,
        })
    }
}
