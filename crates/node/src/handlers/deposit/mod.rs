use std::collections::HashSet;

use tokio::sync::broadcast;
use types::intents::DepositIntent;

pub mod create_deposit;
pub mod handler;

pub struct DepositIntentState {
    pub pending_intents: Vec<DepositIntent>,
    pub deposit_addresses: HashSet<String>,
    pub deposit_intent_tx: broadcast::Sender<DepositIntent>,
    pub processed_txids: HashSet<bitcoin::Txid>,
}
