use bitcoin::Txid;
use esplora_client::AsyncClient;

use crate::errors::NodeError;

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

pub mod mock {
    use super::*;
    use std::collections::HashMap;

    pub struct MockOracle {
        // Map of tx_hash -> (address, amount, is_valid)
        pub transactions: HashMap<String, (String, u64, bool)>,
    }

    impl Default for MockOracle {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockOracle {
        pub fn new() -> Self {
            Self {
                transactions: HashMap::new(),
            }
        }

        pub fn add_transaction(
            &mut self,
            tx_hash: Txid,
            address: String,
            amount: u64,
            is_valid: bool,
        ) {
            self.transactions
                .insert(tx_hash.to_string(), (address, amount, is_valid));
        }
    }

    #[async_trait::async_trait]
    impl Oracle for MockOracle {
        async fn validate_transaction(
            &self,
            address: &str,
            amount: u64,
            tx_hash: Txid,
        ) -> Result<bool, NodeError> {
            let tx_hash_str = tx_hash.to_string();

            match self.transactions.get(&tx_hash_str) {
                Some((expected_address, expected_amount, is_valid)) => {
                    if expected_address != address {
                        return Err(NodeError::Error(format!(
                            "Address mismatch: expected {}, got {}",
                            expected_address, address
                        )));
                    }

                    if *expected_amount != amount {
                        return Err(NodeError::Error(format!(
                            "Amount mismatch: expected {}, got {}",
                            expected_amount, amount
                        )));
                    }

                    Ok(*is_valid)
                }
                None => Err(NodeError::Error("Transaction not found".to_string())),
            }
        }
    }
}
