use std::{collections::HashMap, str::FromStr};

use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Txid};
use protocol::oracle::{Oracle, Utxo};
use types::errors::NodeError;

#[derive(Clone)]
pub struct MockOracle {
    // Map of tx_hash -> (address, amount, is_valid)
    pub transactions: HashMap<String, (String, u64, bool)>,
}

impl Default for MockOracle {
    fn default() -> Self {
        Self::new()
    }
}

impl MockOracle {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
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
}
