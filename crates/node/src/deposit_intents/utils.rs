use std::collections::HashSet;

use tokio::sync::broadcast;
use tracing::error;

use types::errors::NodeError;

use crate::{
    NodeState,
    db::Db,
    deposit_intents::{DepositIntent, DepositIntentState},
    swarm_manager::Network,
};

impl DepositIntentState {
    pub fn new(deposit_intent_tx: broadcast::Sender<String>) -> Self {
        Self {
            pending_intents: vec![],
            deposit_addresses: HashSet::new(),
            deposit_intent_tx,
        }
    }

    pub async fn create_deposit<N: Network, D: Db>(
        &mut self,
        node: &mut NodeState<N, D>,
        deposit_intent: DepositIntent,
    ) -> Result<(), NodeError> {
        node.db.insert_deposit_intent(deposit_intent.clone())?;

        if self
            .deposit_addresses
            .insert(deposit_intent.deposit_address.clone())
        {
            if let Err(e) = self
                .deposit_intent_tx
                .send(deposit_intent.deposit_address.clone())
            {
                error!("Failed to notify deposit monitor of new address: {}", e);
            }
        }

        Ok(())
    }

    pub fn get_pending_deposit_intents(&self) -> Vec<DepositIntent> {
        self.pending_intents.clone()
    }
}
