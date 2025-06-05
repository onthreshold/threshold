use bitcoin::{Address, Transaction, consensus};
use crate::errors::NodeError;
use esplora_client::{Builder, AsyncClient};

pub trait WindowedConfirmedTransactionProvider {
    // Must only return transactions that are confirmed in the given range [min_height, max_height].
    // All returned transactions must have at least six confirmations (< current_chain_tip_height - 6).
    async fn get_confirmed_transactions(
        &self,
        address: Address,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<Transaction>, NodeError>;
}

pub struct EsploraApiClient;

impl WindowedConfirmedTransactionProvider for EsploraApiClient {
    async fn get_confirmed_transactions(&self, address: Address, min_height: u32, max_height: u32) -> Result<Vec<Transaction>, NodeError> {
        let builder = Builder::new("https://blockstream.info/testnet/api");
        let async_client = builder.build_async();

        let blockchain_height = async_client.get_height().await
            .map_err(|e| NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e)))?;

        let new_max_height = max_height.min(blockchain_height - 6);

        let mut confirmed_txs = Vec::new();
        let mut last_seen_txid = None;

        loop {
            let address_txs = async_client.scripthash_txs(&address.script_pubkey().to_string(), last_seen_txid.as_ref())
                .await
                .map_err(|e| NodeError::Error(format!("Cannot retrieve transactions for address: {}", e)))?;

            if address_txs.is_empty() {
                break;
            }

            let mut found_confirmed = false;
            
            for tx in address_txs {
                if let Some(block_height) = tx.status.block_height {
                    if block_height >= min_height && block_height <= new_max_height {
                        if let Ok(full_tx) = async_client.get_tx(&tx.txid.to_string()).await {
                            if let Ok(bitcoin_tx) = consensus::deserialize(&hex::decode(full_tx.hex).unwrap()) {
                                confirmed_txs.push(bitcoin_tx);
                                found_confirmed = true;
                            }
                        }
                    }
                }
                last_seen_txid = Some(tx.txid);
            }

            if !found_confirmed && last_seen_txid.as_ref().map_or(false, |txid| {
                if let Some(height) = address_txs.iter().find(|tx| tx.txid == *txid).and_then(|tx| tx.status.block_height) {
                    height < min_height
                } else {
                    false
                }
            }) {
                break;
            }
        }

        Ok(confirmed_txs)
    }
}