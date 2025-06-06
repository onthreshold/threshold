use types::errors::NodeError;

use crate::{block::BlockBody, transaction::Transaction};

pub struct ProposedBlock {
    block_body: BlockBody,
}

impl Default for ProposedBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl ProposedBlock {
    pub fn new() -> Self {
        Self {
            block_body: BlockBody::new(vec![]),
        }
    }

    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), NodeError> {
        self.block_body.transactions.push(transaction);
        Ok(())
    }
}
