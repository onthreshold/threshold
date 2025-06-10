use std::collections::HashSet;

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

pub mod handler;
pub mod utils;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct DepositIntent {
    pub user_id: String,
    pub amount_sat: u64,
    pub deposit_tracking_id: String,
    pub deposit_address: String,
    pub timestamp: u64,
}

pub struct DepositIntentState {
    pub pending_intents: Vec<DepositIntent>,
    pub deposit_addresses: HashSet<String>,
    pub deposit_intent_tx: broadcast::Sender<String>,
}
