use bitcoin::{ Address, Transaction, consensus, Network };
use std::str::FromStr;
use esplora_client::Builder;
use async_trait::async_trait;

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
}

pub struct EsploraApiClient;


#[async_trait]
impl WindowedConfirmedTransactionProvider for EsploraApiClient {
    async fn get_confirmed_transactions(
        &self,
        address: Address,
        min_height: u32,
        max_height: u32
    ) -> Result<Vec<Transaction>, NodeError> {
        let builder = Builder::new("https://blockstream.info/api");
        let async_client = match builder.build_async() {
            Ok(client) => client,
            Err(e) => return Err(NodeError::Error(format!("Cannot build esplora client: {}", e))),
        };

        let blockchain_height = async_client
            .get_height().await
            .map_err(|e| NodeError::Error(format!("Cannot retrieve height of blockchain: {}", e)))?;

        let new_max_height = max_height.min(blockchain_height - 6);

        let mut confirmed_txs = Vec::new();
        let mut last_seen_txid = None;

        loop {
            let address_txs = async_client
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
                        if let Ok(full_tx) = async_client.get_tx(&tx.txid).await {
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
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = EsploraApiClient;

    let address = Address::from_str("bc1qezwz3yt46nsgzcwlg0dsw680nryjpq5u8pvzts")
        .unwrap()
        .require_network(Network::Bitcoin)
        .unwrap();

    let transactions = runtime.block_on(client.get_confirmed_transactions(address, 899900, 900000)).unwrap();

    println!("Found {} transactions.", transactions.len());
    for tx in transactions {
        println!("Transaction ID: {}", tx.compute_txid());
    }
}