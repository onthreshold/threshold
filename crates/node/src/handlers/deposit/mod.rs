use std::collections::HashSet;

use types::intents::DepositIntent;

pub mod create_deposit;
pub mod handler;

pub struct DepositIntentState {
    pub pending_intents: Vec<DepositIntent>,
    pub deposit_addresses: HashSet<String>,
    pub deposit_intent_tx: crossbeam_channel::Sender<DepositIntent>,
    pub processed_txids: HashSet<bitcoin::Txid>,
}
