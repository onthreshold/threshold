use bitcoin::Txid;
use esplora_client::AsyncClient;
use types::errors::NodeError;

#[async_trait::async_trait]
pub trait Oracle {
    async fn validate_transaction(
        &self,
        address: &str,
        amount: u64,
        tx_hash: Txid,
    ) -> Result<bool, NodeError>;
}

pub struct BitcoinOracle {
    pub esplora_client: AsyncClient,
}

impl BitcoinOracle {
    pub fn new(esplora_client: AsyncClient) -> Self {
        Self { esplora_client }
    }
}

#[async_trait::async_trait]
impl Oracle for BitcoinOracle {
    async fn validate_transaction(
        &self,
        _address: &str,
        amount: u64,
        tx_hash: Txid,
    ) -> Result<bool, NodeError> {
        let tx = self
            .esplora_client
            .get_tx_info(&tx_hash)
            .await
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let tx = tx.ok_or(NodeError::Error("Transaction not found".to_string()))?;

        if !tx.status.confirmed {
            return Err(NodeError::Error("Transaction not confirmed".to_string()));
        }

        let mut total_output = 0;
        for output in tx.vout {
            total_output += output.value;
        }

        if total_output != amount {
            return Err(NodeError::Error(
                "Transaction output value mismatch".to_string(),
            ));
        }

        Ok(true)
    }
}
