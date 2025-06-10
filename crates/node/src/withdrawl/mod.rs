use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod handler;
pub mod withdrawl;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendIntent {
    pub amount_sat: u64,
    pub address_to: String,
    pub public_key: String,
    pub blocks_to_confirm: Option<u32>,
}

pub struct SpendIntentState {
    pub pending_intents: HashMap<String, SpendIntent>,
}
