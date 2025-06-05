use bitcoin::{ Address, Transaction, consensus, Network };
use std::str::FromStr;
use esplora_client::{AsyncClient, Builder};
use async_trait::async_trait;
use tokio::time::{sleep, Duration};
use tokio::sync::broadcast;

#[derive(Debug)]
pub enum NodeError {
    Error(String),
}

#[async_trait]
pub trait WindowedConfirmedTransactionProvider {
    // Must only return transactions that are confirmed in the given range [min_height, max_height].
    // All returned transactions must have at least six confirmations (< current_chain_tip_height - 6).
    async fn get_confirmed_transactions(
        &self,
        address: Address,
        min_height: u32,
        max_height: u32
    ) -> Result<Vec<Transaction>, NodeError>;

    // Must poll for new transactions and send them to the given channel.
    async fn poll_new_transactions(
        &self,
        address: Address,
    );
}

pub struct EsploraApiClient {
    client: AsyncClient,
    tx_channel: broadcast::Sender<Transaction>,
}

impl EsploraApiClient {
    pub fn new(client: AsyncClient, capacity: usize) -> Self {
        Self { client, tx_channel: broadcast::channel(capacity).0}
    }
}


#[async_trait]
impl WindowedConfirmedTransactionProvider for EsploraApiClient {
    async fn get_confirmed_transactions(
        &self,
        address: Address,
        min_height: u32,
        max_height: u32
    ) -> Result<Vec<Transaction>, NodeError> {
        let blockchain_height = self.client
            .get_height().await
            .map_err(|e| NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e)))?;

        let new_max_height = max_height.min(blockchain_height - 6);

        let mut confirmed_txs = Vec::new();
        let mut last_seen_txid = None;

        loop {
            let address_txs = self.client
                .scripthash_txs(&address.script_pubkey(), last_seen_txid).await
                .map_err(|e|
                    NodeError::Error(format!("Cannot retrieve transactions for address: {}", e))
                )?;

            if address_txs.is_empty() {
                break;
            }

            let mut found_confirmed = false;
            let last_tx_height = address_txs.last().and_then(|tx| tx.status.block_height);

            for tx in address_txs {
                if let Some(block_height) = tx.status.block_height {
                    if block_height >= min_height && block_height <= new_max_height {
                        if let Ok(full_tx) = self.client.get_tx(&tx.txid).await {
                            if let Ok(bitcoin_tx) = consensus::deserialize(&consensus::serialize(&full_tx.unwrap())) {
                                confirmed_txs.push(bitcoin_tx);
                                found_confirmed = true;
                            }
                        }
                    }
                }
                last_seen_txid = Some(tx.txid);
            }

            if !found_confirmed && last_tx_height.map_or(false, |height| height < min_height) {
                break;
            }
        }

        Ok(confirmed_txs)
    }

    async fn poll_new_transactions(
        &self,
        address: Address,
    ) {
        let mut last_confirmed_height = match self.client
            .get_height().await {
                Ok(height) => height - 6,
                Err(e) => {
                    eprintln!("Cannot retrieve height of blockchain: {}", e);
                    return;
                }
            };

        println!("Polling for new transactions for address {}, starting from confirmed height {}", address, last_confirmed_height);

        loop {
            sleep(Duration::from_secs(60)).await;

            let current_height = match self.client.get_height().await {
                Ok(height) => height,
                Err(e) => {
                    eprintln!("Cannot retrieve height of blockchain: {}", e);
                    continue;
                }
            };
            
            let new_confirmed_height = current_height - 6;

            if new_confirmed_height > last_confirmed_height {
                println!("New confirmed block found. From height {} to {}", last_confirmed_height + 1, new_confirmed_height);
                
                let new_txs = match self.get_confirmed_transactions(address.clone(), last_confirmed_height + 1, new_confirmed_height).await {
                    Ok(txs) => txs,
                    Err(e) => {
                        eprintln!("Error getting confirmed transactions: {:?}", e);
                        continue;
                    }
                };

                for tx in new_txs {
                    println!("Found new confirmed transaction: {}", tx.compute_txid());
                    match self.tx_channel.send(tx) {
                        Ok(_) => (),
                        Err(e) => {
                            eprintln!("Error sending transaction to channel: {:?}", e);
                        }
                    }
                }
                
                last_confirmed_height = new_confirmed_height;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let client = EsploraApiClient::new(Builder::new("https://blockstream.info/api").build_async().unwrap(), 100);

    let address = Address::from_str("bc1qezwz3yt46nsgzcwlg0dsw680nryjpq5u8pvzts")
        .unwrap()
        .require_network(Network::Bitcoin)
        .unwrap();

    let transactions = client.get_confirmed_transactions(address.clone(), 899900, 900000).await.unwrap();
    println!("Found {} transactions.", transactions.len());
    for tx in transactions {
        println!("Transaction ID: {}", tx.compute_txid());
    }

    client.poll_new_transactions(address).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_confirmed_transactions() {
        let client = EsploraApiClient::new(Builder::new("https://blockstream.info/api").build_async().unwrap(), 100);
        let address = Address::from_str("bc1qezwz3yt46nsgzcwlg0dsw680nryjpq5u8pvzts")
            .unwrap()
            .require_network(Network::Bitcoin)
            .unwrap();
        let transactions = client.get_confirmed_transactions(address.clone(), 899900, 899930).await.unwrap();

        let correct_txs = vec!["99c024e891c3110297513a1bc8c6f36948b36461096e664be72c3ac96e958c5c",  "1d0249929acaf31c2c6b6e6f9c72f44bd663a426cb146afe0b7bbaa66e0bc0df", "fdcd9cf8d660e359a6ab2993d649276fca60be01c2b4327f95ad2527cbe3db08", "3fd280c3ccc13f0f88433f0ce95aeebacc249565c8e8b671005302de0616babe", "a8705186a9d6b5063484a8029b0e2c4064e3e2723ea61ea10b6bc38d0abbc77a"];
        
        assert_eq!(transactions.len(), correct_txs.len());
            
        for tx in transactions {
            assert!(correct_txs.contains(&tx.compute_txid().to_string().as_str()));
        }
    }
}