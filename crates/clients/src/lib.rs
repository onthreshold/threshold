use async_trait::async_trait;
use bitcoin::{consensus, Address, Amount, Network, OutPoint, ScriptBuf, Transaction};
use esplora_client::{AsyncClient, Builder};
use std::{collections::HashSet, str::FromStr};
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

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
    async fn poll_new_transactions(&mut self, addresses: Vec<Address>);
}

#[derive(Debug)]
pub struct EsploraApiClient {
    pub client: AsyncClient,
    pub tx_channel: Option<broadcast::Sender<Transaction>>,
    pub deposit_intent_rx: Option<broadcast::Receiver<String>>,
}

pub struct Utxo {
    pub outpoint: OutPoint,
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}

impl Default for EsploraApiClient {
    fn default() -> Self {
        let builder = Builder::new("https://blockstream.info/api");
        let client = builder.build_async().unwrap();
        Self::new(client, None, None)
    }
}

impl EsploraApiClient {
    pub fn new(
        client: AsyncClient,
        capacity: Option<usize>,
        deposit_intent_rx: Option<broadcast::Receiver<String>>,
    ) -> Self {
        Self {
            client,
            tx_channel: capacity.map(|c| broadcast::channel(c).0),
            deposit_intent_rx,
        }
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

                last_seen_txid = Some(address_txs.last().unwrap().txid);

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
            }

            sleep(Duration::from_secs(5)).await;
        }

        Ok(confirmed_txs)
    }

    async fn poll_new_transactions(&mut self, addresses: Vec<Address>) {
        let mut last_confirmed_height = match self.client.get_height().await {
            Ok(height) => height - 6,
            Err(e) => {
                error!("Cannot retrieve height of blockchain: {}", e);
                return;
            }
        };

        info!(
            "Polling for new transactions, starting from confirmed height {}",
            last_confirmed_height
        );

        let mut deposit_intent_rx = self.deposit_intent_rx.take().unwrap();
        let mut addresses: HashSet<_> = addresses.into_iter().collect();

        loop {
            tokio::select! {
                _ = sleep(Duration::from_secs(30)) => {
                    let current_height = match self.client.get_height().await {
                        Ok(height) => height,
                        Err(e) => {
                            error!("Cannot retrieve height of blockchain: {}", e);
                            continue;
                        }
                    };

                    let new_confirmed_height = current_height - 6;

                    if new_confirmed_height > last_confirmed_height {
                        info!(
                            "New confirmed block found. From height {} to {}",
                            last_confirmed_height + 1,
                            new_confirmed_height
                        );

                        let new_txs = match self
                            .get_confirmed_transactions(
                                addresses.iter().cloned().collect(),
                                last_confirmed_height + 1,
                                new_confirmed_height,
                            )
                            .await
                        {
                            Ok(txs) => txs,
                            Err(e) => {
                                error!("Error getting confirmed transactions: {:?}", e);
                                continue;
                            }
                        };

                        for tx in new_txs {
                            println!("Found new confirmed transaction: {}", tx.compute_txid());
                            match self.tx_channel.as_ref().unwrap().send(tx) {
                                Ok(_) => (),
                                Err(e) => {
                                    error!("Error sending transaction to channel: {:?}", e);
                                }
                            }
                        }

                        last_confirmed_height = new_confirmed_height;
                    }
                }
                Ok(address_str) = deposit_intent_rx.recv() => {
                    info!("Received new deposit address to monitor: {}", &address_str);
                    if addresses.insert(
                        Address::from_str(&address_str)
                            .unwrap()
                            .require_network(Network::Bitcoin)
                            .unwrap(),
                    ) {
                        info!("Now polling {} addresses.", addresses.len());
                    }
                }
            }
        }
    }
}
