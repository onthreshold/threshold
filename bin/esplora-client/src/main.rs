use bitcoin::{ Address, Transaction, consensus, Network };
use std::str::FromStr;
use esplora_client::{AsyncClient, Builder};
use async_trait::async_trait;
use tokio::time::{sleep, Duration};

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
    ) -> Result<(), NodeError>;
}

pub struct EsploraApiClient {
    client: AsyncClient,
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
    ) -> Result<(), NodeError> {
        let mut last_known_height = self.client
            .get_height().await
            .map_err(|e| NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e)))?;
        
        println!("Polling for new transactions for address {}, starting from height {}", address, last_known_height);

        loop {
            sleep(Duration::from_secs(60)).await;

            let current_height = self.client
                .get_height().await
                .map_err(|e| NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e)))?;
            
            if current_height > last_known_height {
                println!("New block(s) detected. Current height: {}", current_height);
                // We check for transactions with at least 6 confirmations.
                let new_max_height = current_height.saturating_sub(6);

                let mut last_seen_txid = None;
                'fetch_loop: loop {
                    let address_txs = self.client
                        .scripthash_txs(&address.script_pubkey(), last_seen_txid).await
                        .map_err(|e|
                            NodeError::Error(format!("Cannot retrieve transactions for address: {}", e))
                        )?;

                    if address_txs.is_empty() {
                        break 'fetch_loop;
                    }
                    
                    last_seen_txid = address_txs.last().map(|tx| tx.txid);

                    for tx_info in address_txs {
                        if let Some(block_height) = tx_info.status.block_height {
                            if block_height > last_known_height && block_height <= new_max_height {
                                // This is a new confirmed transaction.
                                if let Ok(Some(full_tx)) = self.client.get_tx(&tx_info.txid).await {
                                    if let Ok(bitcoin_tx) = consensus::deserialize::<Transaction>(&consensus::serialize(&full_tx)) {
                                        println!("Found new confirmed transaction: {}", bitcoin_tx.compute_txid());
                                    }
                                }
                            } else if block_height <= last_known_height {
                                // Since transactions are in reverse chronological order, we can stop fetching more pages.
                                break 'fetch_loop;
                            }
                        }
                    }
                }
                
                last_known_height = current_height;
            }
        }
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let client = EsploraApiClient {
            client: Builder::new("https://blockstream.info/api").build_async().unwrap(),
        };

        let address = Address::from_str("bc1qezwz3yt46nsgzcwlg0dsw680nryjpq5u8pvzts")
            .unwrap()
            .require_network(Network::Bitcoin)
            .unwrap();

        if let Err(e) = client.poll_new_transactions(address).await {
            eprintln!("Polling failed: {:?}", e);
        }
    });
}