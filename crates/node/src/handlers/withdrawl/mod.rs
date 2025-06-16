use std::collections::HashMap;

use types::intents::SpendIntent;

pub mod create_withdrawl;
pub mod handler;

pub struct SpendIntentState {
    pub pending_intents: HashMap<String, (SpendIntent, u64)>,
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
