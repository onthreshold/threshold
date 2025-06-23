use std::{collections::HashMap, str::FromStr};

use crate::oracle::Oracle;
use bitcoin::{
    Address, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    absolute::LockTime, hashes::Hash, transaction::Version,
};
use tokio::sync::broadcast;
use tracing::{error, info};
use types::{
    errors::NodeError,
    intents::DepositIntent,
    network::network_event::{NetworkEvent, SelfRequest},
    utxo::Utxo,
};

#[derive(Clone)]
pub struct MockOracle {
    // Map of tx_hash -> (address, amount, is_valid)
    pub transactions: HashMap<String, (String, u64, bool)>,
    pub tx_channel: broadcast::Sender<NetworkEvent>,
    pub deposit_intent_rx: Option<broadcast::Sender<DepositIntent>>,
}

impl MockOracle {
    #[must_use]
    pub fn new(
        tx_channel: broadcast::Sender<NetworkEvent>,
        deposit_intent_rx: Option<broadcast::Sender<DepositIntent>>,
    ) -> Self {
        Self {
            transactions: HashMap::new(),
            tx_channel,
            deposit_intent_rx,
        }
    }

    pub fn add_transaction(&mut self, tx_hash: Txid, address: String, amount: u64, is_valid: bool) {
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
        if self.transactions.is_empty() {
            return Ok(true);
        }

        let tx_hash_str = tx_hash.to_string();

        match self.transactions.get(&tx_hash_str) {
            Some((expected_address, expected_amount, is_valid)) => {
                if expected_address != address {
                    return Err(NodeError::Error(format!(
                        "Address mismatch: expected {expected_address}, got {address}"
                    )));
                }

                if *expected_amount != amount {
                    return Err(NodeError::Error(format!(
                        "Amount mismatch: expected {expected_amount}, got {amount}"
                    )));
                }

                Ok(*is_valid)
            }
            None => Err(NodeError::Error("Transaction not found".to_string())),
        }
    }

    async fn get_current_fee_per_vb(&self, priority: Option<u16>) -> Result<f64, NodeError> {
        if priority.is_some() {
            Ok(100.0)
        } else {
            Ok(10.0)
        }
    }

    async fn refresh_utxos(
        &self,
        _address: Address,
        _number_pages: u32,
        _start_transactions: Option<Txid>,
        _allow_unconfirmed: bool,
    ) -> Result<Vec<Utxo>, NodeError> {
        Ok(vec![
            Utxo {
                outpoint: OutPoint::new(
                    Txid::from_str(
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    )
                    .unwrap(),
                    0,
                ),
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new(),
            },
            Utxo {
                outpoint: OutPoint::new(
                    Txid::from_str(
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    )
                    .unwrap(),
                    0,
                ),
                value: Amount::from_sat(10000),
                script_pubkey: ScriptBuf::new(),
            },
            Utxo {
                outpoint: OutPoint::new(
                    Txid::from_str(
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    )
                    .unwrap(),
                    0,
                ),
                value: Amount::from_sat(100_000),
                script_pubkey: ScriptBuf::new(),
            },
        ])
    }

    async fn broadcast_transaction(&self, _tx: &bitcoin::Transaction) -> Result<String, NodeError> {
        Ok(String::new())
    }

    async fn get_confirmed_transactions(
        &self,
        _addresses: Vec<Address>,
        _min_height: u32,
        _max_height: u32,
    ) -> Result<Vec<bitcoin::Transaction>, NodeError> {
        Ok(vec![])
    }

    async fn poll_new_transactions(&mut self, _addresses: Vec<Address>) {
        info!("Polling new transactions");
        let Some(dep_tx_sender) = self.deposit_intent_rx.take() else {
            return;
        };

        let mut deposit_rx = dep_tx_sender.subscribe();

        loop {
            match deposit_rx.recv().await {
                Ok(deposit_intent) => {
                    info!("Received new address: {}", deposit_intent.deposit_address);
                    if let Ok(addr) = Address::from_str(&deposit_intent.deposit_address) {
                        let tx = Self::create_dummy_tx(
                            &addr.assume_checked(),
                            deposit_intent.amount_sat,
                        );

                        // Add transaction to internal HashMap for later validation
                        self.add_transaction(
                            tx.compute_txid(),
                            deposit_intent.user_pubkey.clone(),
                            deposit_intent.amount_sat,
                            true,
                        );

                        info!(
                            "Added transaction {} to oracle for user {} with amount {}",
                            tx.compute_txid(),
                            deposit_intent.user_pubkey,
                            deposit_intent.amount_sat
                        );

                        if let Err(e) = self.tx_channel.send(NetworkEvent::SelfRequest {
                            request: SelfRequest::ConfirmDeposit { confirmed_tx: tx },
                            response_channel: None,
                        }) {
                            error!("Failed to send dummy tx: {}", e);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => (), // skip missed messages
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn get_latest_block_height(&self) -> Result<u32, NodeError> {
        // Return a constant dummy height
        Ok(0)
    }

    async fn get_transaction_by_address(&self, _tx_id: &str) -> Result<Transaction, NodeError> {
        let tx = Self::create_dummy_tx_without_address(1000);
        Ok(tx)
    }
}

impl MockOracle {
    #[must_use]
    pub fn create_dummy_tx(address: &Address, value_sat: u64) -> Transaction {
        let tx_in = TxIn {
            previous_output: OutPoint {
                txid: Txid::from_slice(&[0u8; 32]).unwrap(),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ZERO,
            witness: bitcoin::witness::Witness::new(),
        };

        let tx_out = TxOut {
            value: Amount::from_sat(value_sat),
            script_pubkey: address.script_pubkey(),
        };

        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![tx_in],
            output: vec![tx_out],
        }
    }

    #[must_use]
    pub fn create_dummy_tx_without_address(value_sat: u64) -> Transaction {
        Self::create_dummy_tx(
            &Address::from_str("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh")
                .unwrap()
                .assume_checked(),
            value_sat,
        )
    }
}
