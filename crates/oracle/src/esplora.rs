use crate::oracle::Oracle;
use bitcoin::{consensus, Address, Amount, Network, OutPoint, Transaction, Txid};
use esplora_client::{AsyncClient, Builder};
use std::{collections::HashSet, str::FromStr};
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use types::{
    errors::NodeError,
    intents::DepositIntent,
    network_event::{NetworkEvent, SelfRequest},
    utxo::Utxo,
};

#[derive(Clone)]
pub struct EsploraOracle {
    pub client: AsyncClient,
    pub tx_channel: crossbeam_channel::Sender<NetworkEvent>,
    pub deposit_intent_rx: Option<crossbeam_channel::Receiver<DepositIntent>>,
    pub confirmation_depth: u32,
    pub monitor_start_block: i32,
}

impl EsploraOracle {
    pub fn new(
        network: Network,
        capacity: Option<usize>,
        tx_channel: Option<crossbeam_channel::Sender<NetworkEvent>>,
        deposit_intent_rx: Option<crossbeam_channel::Receiver<DepositIntent>>,
        confirmation_depth: u32,
        monitor_start_block: i32,
    ) -> Self {
        let url = match network {
            Network::Bitcoin => "https://blockstream.info/api",
            Network::Testnet => "https://blockstream.info/testnet/api",
            Network::Signet => "https://blockstream.info/signet/api",
            Network::Regtest => panic!("Regtest network is not supported by Esplora"),
            _ => panic!("Unsupported network type"),
        };
        let builder = Builder::new(url);
        let client = builder.build_async().unwrap();
        Self {
            client,
            tx_channel: tx_channel
                .unwrap_or(crossbeam_channel::bounded(capacity.unwrap_or(1000)).0),
            deposit_intent_rx,
            confirmation_depth,
            monitor_start_block,
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
            .client
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
            .client
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
                .client
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
                let Some(full_tx) = self.client.get_tx(&tx.txid).await.ok().flatten() else {
                    continue;
                };
                let Ok(tx_status) = self.client.get_tx_status(&tx.txid).await else {
                    continue;
                };
                if !allow_unconfirmed && !tx_status.confirmed {
                    continue;
                }

                for (vout, output) in full_tx.output.iter().enumerate() {
                    if output.script_pubkey != script {
                        continue;
                    }
                    let Ok(Some(output_status)) =
                        self.client.get_output_status(&tx.txid, vout as u64).await
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
        self.client
            .broadcast(tx)
            .await
            .map_err(|e| NodeError::Error(format!("Failed to broadcast transaction: {}", e)))?;

        Ok(tx_hex)
    }

    async fn get_confirmed_transactions(
        &self,
        addresses: Vec<Address>,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<Transaction>, NodeError> {
        let blockchain_height = self.client.get_height().await.map_err(|e| {
            NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e))
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

        let deposit_intent_rx = self.deposit_intent_rx.take().unwrap();
        let mut addresses: HashSet<_> = addresses.into_iter().collect();

        println!("monitor_start_block: {}", self.monitor_start_block);

        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            // Drain all pending deposit intents (non-blocking)
            while let Ok(deposit_intent) = deposit_intent_rx.try_recv() {
                info!(
                    "Received new deposit address to monitor: {}",
                    &deposit_intent.deposit_address
                );
                if addresses.insert(
                    Address::from_str(&deposit_intent.deposit_address)
                        .unwrap()
                        .assume_checked(),
                ) {
                    info!("Now polling {} addresses.", addresses.len());
                }
            }

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
                    self.monitor_start_block as u32
                } else {
                    last_confirmed_height + 1
                };

                info!(
                    "New confirmed block found. Now monitoring from height {} to {}",
                    min_height, new_confirmed_height
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
                    if let Err(e) = self.tx_channel.send(NetworkEvent::SelfRequest {
                        request: SelfRequest::ConfirmDeposit { confirmed_tx: tx },
                        response_channel: None,
                    }) {
                        error!("Error sending transaction to channel: {:?}", e);
                    }
                }

                last_confirmed_height = new_confirmed_height;
            }
        }
    }

    async fn get_latest_block_height(&self) -> Result<u32, NodeError> {
        let height = self.client.get_height().await.map_err(|e| {
            NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e))
        })?;
        Ok(height)
    }
}
