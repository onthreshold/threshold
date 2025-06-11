use std::collections::HashSet;

use bincode::{Decode, Encode};
use bitcoin::Transaction;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

pub mod create_deposit;
pub mod handler;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct DepositIntent {
    pub amount_sat: u64,
    pub deposit_tracking_id: String,
    pub deposit_address: String,
    pub timestamp: u64,
}

pub struct DepositIntentState {
    pub pending_intents: Vec<DepositIntent>,
    pub deposit_addresses: HashSet<String>,
    pub deposit_intent_tx: broadcast::Sender<String>,
    pub transaction_rx: broadcast::Receiver<Transaction>,
    pub processed_txids: HashSet<bitcoin::Txid>,
}
