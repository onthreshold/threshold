use std::{collections::HashMap, str::FromStr};

use crate::oracle::Oracle;
use bitcoin::{
    absolute::LockTime, hashes::Hash, transaction::Version, Address, Amount, OutPoint, ScriptBuf,
    Sequence, Transaction, TxIn, TxOut, Txid,
};
use tokio::sync::broadcast;
use tracing::{error, info};
use types::{
    errors::NodeError,
    intents::DepositIntent,
    network_event::{NetworkEvent, SelfRequest},
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
        let tx_hash_str = tx_hash.to_string();

        match self.transactions.get(&tx_hash_str) {
            Some((expected_address, expected_amount, is_valid)) => {
                if expected_address != address {
                    return Err(NodeError::Error(format!(
                        "Address mismatch: expected {}, got {}",
                        expected_address, address
                    )));
                }

                if *expected_amount != amount {
                    return Err(NodeError::Error(format!(
                        "Amount mismatch: expected {}, got {}",
                        expected_amount, amount
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
                value: Amount::from_sat(100000),
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
                        let tx =
                            self.create_dummy_tx(addr.assume_checked(), deposit_intent.amount_sat);
                        if let Err(e) = self.tx_channel.send(NetworkEvent::SelfRequest {
                            request: SelfRequest::ConfirmDeposit { confirmed_tx: tx },
                            response_channel: None,
                        }) {
                            error!("Failed to send dummy tx: {}", e);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue, // skip missed messages
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn get_latest_block_height(&self) -> Result<u32, NodeError> {
        // Return a constant dummy height
        Ok(0)
    }
}

impl MockOracle {
    fn create_dummy_tx(&self, address: Address, value_sat: u64) -> Transaction {
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
}
