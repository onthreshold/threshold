use async_trait::async_trait;
use bitcoin::{consensus, Address, Transaction};
use esplora_client::AsyncClient;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::Mutex as TokioMutex;
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
        addresses: Vec<Address>,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<Transaction>, NodeError>;

    // Must poll for new transactions and send them to the given channel.
    async fn poll_new_transactions(&self, addresses: Vec<Address>);
}

#[derive(Debug, Clone)]
pub struct EsploraApiClient {
    client: AsyncClient,
    tx_channel: broadcast::Sender<Transaction>,
    addresses: Arc<TokioMutex<Vec<Address>>>,
}

impl EsploraApiClient {
    pub fn new(client: AsyncClient, capacity: usize) -> Self {
        Self {
            client,
            tx_channel: broadcast::channel(capacity).0,
            addresses: Arc::new(TokioMutex::new(Vec::new())),
        }
    }

    pub async fn update_addresses(&self, new_addresses: Vec<Address>) {
        let mut addresses = self.addresses.lock().await;
        *addresses = new_addresses;
    }
}

#[async_trait]
impl WindowedConfirmedTransactionProvider for EsploraApiClient {
    async fn get_confirmed_transactions(
        &self,
        addresses: Vec<Address>,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<Transaction>, NodeError> {
        let blockchain_height = self.client.get_height().await.map_err(|e| {
            NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e))
        })?;

        let new_max_height = max_height.min(blockchain_height - 6);
        let mut confirmed_txs = Vec::new();

        for address in &addresses {
            let mut last_seen_txid = None;

            loop {
                let address_txs = self
                    .client
                    .scripthash_txs(&address.script_pubkey(), last_seen_txid)
                    .await
                    .map_err(|e| {
                        NodeError::Error(format!("Cannot retrieve transactions for address: {}", e))
                    })?;

                if address_txs.is_empty() {
                    break;
                }

                let mut found_confirmed = false;
                let last_tx_height = address_txs.last().and_then(|tx| tx.status.block_height);

                for tx in address_txs {
                    if let Some(block_height) = tx.status.block_height {
                        if block_height >= min_height && block_height <= new_max_height {
                            if let Ok(full_tx) = self.client.get_tx(&tx.txid).await {
                                if let Ok(bitcoin_tx) =
                                    consensus::deserialize(&consensus::serialize(&full_tx.unwrap()))
                                {
                                    confirmed_txs.push(bitcoin_tx);
                                    found_confirmed = true;
                                }
                            }
                        }
                    }
                }

                if !found_confirmed && last_tx_height.is_some_and(|height| height < min_height) {
                    break;
                }

                sleep(Duration::from_secs(5)).await;
            }
        }

        Ok(confirmed_txs)
    }

    async fn poll_new_transactions(&self, addresses: Vec<Address>) {
        self.update_addresses(addresses).await;

        let mut last_confirmed_height = match self.client.get_height().await {
            Ok(height) => height - 6,
            Err(e) => {
                eprintln!("Cannot retrieve height of blockchain: {}", e);
                return;
            }
        };

        println!(
            "Polling for new transactions, starting from confirmed height {}",
            last_confirmed_height
        );

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
                println!(
                    "New confirmed block found. From height {} to {}",
                    last_confirmed_height + 1,
                    new_confirmed_height
                );

                let addresses = self.addresses.lock().await.clone();

                let new_txs = match self
                    .get_confirmed_transactions(
                        addresses,
                        last_confirmed_height + 1,
                        new_confirmed_height,
                    )
                    .await
                {
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
