use bitcoin::Transaction;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Txid};
use esplora_client::{AsyncClient, Builder};
use types::errors::NodeError;

#[derive(Debug, Clone)]
pub struct Utxo {
    pub outpoint: OutPoint,
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}

#[async_trait::async_trait]
pub trait Oracle: Send + Default + Clone + Sync {
    async fn validate_transaction(
        &self,
        address: &str,
        amount: u64,
        tx_hash: Txid,
    ) -> Result<bool, NodeError>;

    async fn get_current_fee_per_vb(&self, priority: Option<u16>) -> Result<f64, NodeError>;
    async fn refresh_utxos(
        &self,
        address: Address,
        number_pages: u32,
        start_transactions: Option<Txid>,
        allow_unconfirmed: bool,
    ) -> Result<Vec<Utxo>, NodeError>;

    async fn broadcast_transaction(&self, tx: &bitcoin::Transaction) -> Result<String, NodeError>;
}

#[derive(Clone)]
pub struct EsploraOracle {
    pub esplora_client: AsyncClient,
}

impl Default for EsploraOracle {
    fn default() -> Self {
        Self::new(false)
    }
}

impl EsploraOracle {
    pub fn new(is_testnet: bool) -> Self {
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

    async fn refresh_utxos(
        &self,
        address: Address,
        number_pages: u32,
        start_transactions: Option<Txid>,
        allow_unconfirmed: bool,
    ) -> Result<Vec<Utxo>, NodeError> {
        let mut unspent_txs = Vec::new();
        let mut last_seen_txid = start_transactions;
        let script = address.script_pubkey();

        for _ in 0..number_pages {
            let address_txs = self
                .esplora_client
                .scripthash_txs(&script, last_seen_txid)
                .await
                .map_err(|e| {
                    NodeError::Error(format!("Cannot retrieve transactions for address: {}", e))
                })?;

            if address_txs.is_empty() {
                break;
            }

            last_seen_txid = Some(address_txs.last().unwrap().txid);
            for tx in address_txs {
                let Some(full_tx) = self.esplora_client.get_tx(&tx.txid).await.ok().flatten()
                else {
                    continue;
                };
                let Ok(tx_status) = self.esplora_client.get_tx_status(&tx.txid).await else {
                    continue;
                };
                if !allow_unconfirmed && !tx_status.confirmed {
                    continue;
                }

                for (vout, output) in full_tx.output.iter().enumerate() {
                    if output.script_pubkey != script {
                        continue;
                    }
                    let Ok(Some(output_status)) = self
                        .esplora_client
                        .get_output_status(&tx.txid, vout as u64)
                        .await
                    else {
                        continue;
                    };
                    if output_status.spent {
                        continue;
                    }
                    unspent_txs.push(Utxo {
                        outpoint: OutPoint {
                            txid: tx.txid,
                            vout: vout as u32,
                        },
                        value: Amount::from_sat(output.value.to_sat()),
                        script_pubkey: script.clone(),
                    });
                }
            }

            if last_seen_txid.is_none() {
                break;
            }
        }

        Ok(unspent_txs)
    }

    async fn broadcast_transaction(&self, tx: &Transaction) -> Result<String, NodeError> {
        // Serialize the transaction to raw bytes
        let tx_bytes = bitcoin::consensus::encode::serialize(tx);
        let tx_hex = hex::encode(&tx_bytes);

        // Broadcast the transaction
        self.esplora_client
            .broadcast(tx)
            .await
            .map_err(|e| NodeError::Error(format!("Failed to broadcast transaction: {}", e)))?;

        Ok(tx_hex)
    }
}
