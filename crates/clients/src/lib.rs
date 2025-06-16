use async_trait::async_trait;
use bitcoin::{consensus, Address, Network, Transaction};
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
    // All returned transactions must have at least `confirmation_depth` confirmations (< current_chain_tip_height - confirmation_depth).
    async fn get_confirmed_transactions(
        &self,
        addresses: Vec<Address>,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<Transaction>, NodeError>;

    // Must poll for new transactions and send them to the given channel.
    async fn poll_new_transactions(&mut self, addresses: Vec<Address>);

    async fn get_latest_block_height(&self) -> Result<u32, NodeError>;
}

#[derive(Debug)]
pub struct EsploraApiClient {
    pub client: AsyncClient,
    pub tx_channel: broadcast::Sender<Transaction>,
    pub deposit_intent_rx: Option<broadcast::Receiver<String>>,
    pub confirmation_depth: u32,
    pub monitor_start_block: i32,
}

impl Default for EsploraApiClient {
    fn default() -> Self {
        let builder = Builder::new("https://blockstream.info/testnet/api");
        let client = builder.build_async().unwrap();
        Self::new(client, None, None, None, 6, 0)
    }
}

impl EsploraApiClient {
    #[must_use]
    pub fn new(
        client: AsyncClient,
        capacity: Option<usize>,
        tx_channel: Option<broadcast::Sender<Transaction>>,
        deposit_intent_rx: Option<broadcast::Receiver<String>>,
        confirmation_depth: u32,
        monitor_start_block: i32,
    ) -> Self {
        Self {
            client,
            tx_channel: tx_channel.unwrap_or_else(|| broadcast::channel(capacity.unwrap_or(1000)).0),
            deposit_intent_rx,
            confirmation_depth,
            monitor_start_block,
        }
    }

    #[must_use]
    pub fn new_with_network(
        network: Network,
        capacity: Option<usize>,
        tx_channel: Option<broadcast::Sender<Transaction>>,
        deposit_intent_rx: Option<broadcast::Receiver<String>>,
        confirmation_depth: u32,
        monitor_start_block: i32,
    ) -> Self {
        let url = match network {
            Network::Bitcoin => "https://blockstream.info/api",
            Network::Testnet => "https://blockstream.info/testnet/api",
            Network::Signet => "https://blockstream.info/signet/api",
            Network::Regtest => {
                error!("Regtest network is not supported by Esplora");
                return Self::default();
            }
            _ => {
                error!("Unsupported network type");
                return Self::default();
            }
        };
        let builder = Builder::new(url);
        let client = builder.build_async().unwrap();
        Self::new(
            client,
            capacity,
            tx_channel,
            deposit_intent_rx,
            confirmation_depth,
            monitor_start_block,
        )
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
        let blockchain_height =
            self.client.get_height().await.map_err(|e| {
                NodeError::Error(format!("Cannot retrieve height of blockchain: {e}"))
            })?;

        let new_max_height = max_height.min(blockchain_height - self.confirmation_depth);
        let mut confirmed_txs = Vec::new();

        for address in &addresses {
            let mut last_seen_txid = None;

            loop {
                let address_txs = self
                    .client
                    .scripthash_txs(&address.script_pubkey(), last_seen_txid)
                    .await
                    .map_err(|e| {
                        NodeError::Error(format!("Cannot retrieve transactions for address: {e}"))
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
        let confirmation_depth = self.confirmation_depth;

        let mut last_confirmed_height = match self.client.get_height().await {
            Ok(height) => height - confirmation_depth,
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

        println!("monitor_start_block: {}", self.monitor_start_block);

        loop {
            tokio::select! {
                () = sleep(Duration::from_secs(30)) => {
                    let current_height = match self.client.get_height().await {
                        Ok(height) => height,
                        Err(e) => {
                            error!("Cannot retrieve height of blockchain: {}", e);
                            continue;
                        }
                    };
                    tracing::info!("Current height: {}", current_height);

                    let new_confirmed_height = current_height - confirmation_depth;

                    if new_confirmed_height > last_confirmed_height {
                        let min_height: u32 = if self.monitor_start_block >= 0 {
                            u32::try_from(self.monitor_start_block).unwrap()
                        } else {
                            last_confirmed_height + 1
                        };

                        info!(
                            "New confirmed block found. Now monitoring from height {} to {}",
                            min_height,
                            new_confirmed_height
                        );

                        let new_txs = match self
                            .get_confirmed_transactions(
                                addresses.iter().cloned().collect(),
                                min_height,
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
                            match self.tx_channel.send(tx) {
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
                            .assume_checked()
                    ) {
                        info!("Now polling {} addresses.", addresses.len());
                    }
                }
            }
        }
    }

    async fn get_latest_block_height(&self) -> Result<u32, NodeError> {
        let height =
            self.client.get_height().await.map_err(|e| {
                NodeError::Error(format!("Cannot retrieve height of blockchain: {e}"))
            })?;
        Ok(height)
    }
}
