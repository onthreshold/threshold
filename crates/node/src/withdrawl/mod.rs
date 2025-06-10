use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod create_withdrawl;
pub mod handler;

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

impl Default for SpendIntentState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpendIntentState {
    pub fn new() -> Self {
        Self {
            pending_intents: HashMap::new(),
        }
    }
}
