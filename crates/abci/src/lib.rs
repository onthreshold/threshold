use frost_secp256k1::keys::PublicKeyPackage;
use protocol::{
    block::{ChainConfig, GenesisBlock, ValidatorInfo},
    transaction::Transaction,
};
use types::{errors::NodeError, intents::DepositIntent};

use crate::{chain_state::Account, db::Db, executor::TransactionExecutor};

pub mod chain_state;
pub mod db;
pub mod executor;

#[async_trait::async_trait]
pub trait ChainInterface: Send + Sync {
    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError>;
    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError>;
    fn get_deposit_intent_by_address(&self, address: &str) -> Option<&DepositIntent>;

    fn create_genesis_block(
        &mut self,
        validators: Vec<ValidatorInfo>,
        chain_config: ChainConfig,
        pubkey: &PublicKeyPackage,
    ) -> Result<(), NodeError>;

    async fn execute_transaction(&mut self, transaction: Transaction) -> Result<(), NodeError>;
    fn get_account(&self, address: &str) -> Option<&Account>;
}

pub struct ChainInterfaceImpl {
    db: Box<dyn Db>,
    executor: Box<dyn TransactionExecutor>,
    chain_state: chain_state::ChainState,
}

impl ChainInterfaceImpl {
    #[must_use]
    pub fn new(db: Box<dyn Db>, executor: Box<dyn TransactionExecutor>) -> Self {
        Self {
            db,
            executor,
            chain_state: chain_state::ChainState::new(),
        }
    }
}

#[async_trait::async_trait]
impl ChainInterface for ChainInterfaceImpl {
    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError> {
        self.chain_state.insert_deposit_intent(intent.clone());
        self.db.insert_deposit_intent(intent)
    }

    fn get_account(&self, address: &str) -> Option<&Account> {
        self.chain_state.get_account(address)
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        Ok(self.chain_state.get_all_deposit_intents())
    }

    fn get_deposit_intent_by_address(&self, address: &str) -> Option<&DepositIntent> {
        self.chain_state.get_deposit_intent_by_address(address)
    }

    fn create_genesis_block(
        &mut self,
        validators: Vec<ValidatorInfo>,
        chain_config: ChainConfig,
        pubkey: &PublicKeyPackage,
    ) -> Result<(), NodeError> {
        let genesis_block = GenesisBlock::new(
            validators,
            chain_config,
            pubkey
                .serialize()
                .map_err(|e| NodeError::Error(format!("Failed to serialize public key: {e}")))?,
        );
        self.db.insert_block(genesis_block.to_block())
    }

    async fn execute_transaction(&mut self, transaction: Transaction) -> Result<(), NodeError> {
        let new_chain_state = self
            .executor
            .execute_transaction(transaction, self.chain_state.clone())
            .await?;

        self.db.flush_state(&new_chain_state)?;
        self.chain_state = new_chain_state;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
