use bitcoin::Txid;
use esplora_client::{AsyncClient, Builder};
use types::errors::NodeError;

#[async_trait::async_trait]
pub trait Oracle: Send {
    async fn validate_transaction(
        &self,
        address: &str,
        amount: u64,
        tx_hash: Txid,
    ) -> Result<bool, NodeError>;

    async fn get_current_fee_per_vb(&self, priority: Option<u16>) -> Result<f64, NodeError>;
}

pub struct EsploraOracle {
    pub esplora_client: AsyncClient,
}

impl Default for EsploraOracle {
    fn default() -> Self {
        Self::new()
    }
}

impl EsploraOracle {
    pub fn new() -> Self {
        dotenvy::dotenv().ok();
        let is_testnet: bool = std::env::var("IS_TESTNET")
            .unwrap_or("false".to_string())
            .parse()
            .unwrap();
        let builder = Builder::new(if is_testnet {
            "https://blockstream.info/testnet/api"
        } else {
            "https://blockstream.info/api"
        });
        let async_client = builder.build_async().unwrap();

        Self {
            esplora_client: async_client,
        }
    }
}

#[async_trait::async_trait]
impl Oracle for EsploraOracle {
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

    async fn get_current_fee_per_vb(&self, priority: Option<u16>) -> Result<f64, NodeError> {
        let fee = self
            .esplora_client
            .get_fee_estimates()
            .await
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let priority = priority.unwrap_or(3);

        let fee = fee
            .get(&priority)
            .ok_or(NodeError::Error("Fee not found".to_string()))?;

        Ok(*fee)
    }
}
