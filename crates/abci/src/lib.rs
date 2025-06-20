use frost_secp256k1::keys::PublicKeyPackage;
use protocol::{
    block::{Block, ChainConfig, GenesisBlock, ValidatorInfo},
    transaction::Transaction,
};
use tokio::sync::broadcast;
use types::{errors::NodeError, intents::DepositIntent};

use crate::{chain_state::Account, db::Db, executor::TransactionExecutor};

pub mod chain_state;
pub mod db;
pub mod executor;
pub mod main_loop;

#[async_trait::async_trait]
pub trait ChainInterface: Send + Sync {
    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError>;
    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError>;
    fn get_deposit_intent_by_address(&self, address: &str) -> Option<DepositIntent>;

    fn create_genesis_block(
        &mut self,
        validators: Vec<ValidatorInfo>,
        chain_config: ChainConfig,
        pubkey: &PublicKeyPackage,
    ) -> Result<(), NodeError>;

    async fn add_transaction_to_block(&mut self, transaction: Transaction)
    -> Result<(), NodeError>;
    fn get_account(&self, address: &str) -> Option<Account>;
    fn get_proposed_block(
        &self,
        previous_block: Option<Block>,
        proposer: Vec<u8>,
    ) -> Result<Block, NodeError>;
}

#[derive(Clone)]
pub enum ChainMessage {
    InsertDepositIntent {
        intent: DepositIntent,
    },
    GetAccount {
        address: String,
    },
    GetAllDepositIntents,
    GetDepositIntentByAddress {
        address: String,
    },
    CreateGenesisBlock {
        validators: Vec<ValidatorInfo>,
        chain_config: ChainConfig,
        pubkey: PublicKeyPackage,
    },
    AddTransactionToBlock {
        transaction: Transaction,
    },
    GetProposedBlock {
        previous_block: Option<Block>,
        proposer: Vec<u8>,
    },
}

#[derive(Clone)]
pub enum ChainResponse {
    InsertDepositIntent { error: Option<NodeError> },
    GetAccount { account: Option<Account> },
    GetAllDepositIntents { intents: Vec<DepositIntent> },
    GetDepositIntentByAddress { intent: Option<DepositIntent> },
    CreateGenesisBlock { error: Option<NodeError> },
    AddTransactionToBlock { error: Option<NodeError> },
    GetProposedBlock { block: Block },
}

pub struct ChainInterfaceImpl {
    db: Box<dyn Db>,
    executor: Box<dyn TransactionExecutor>,
    chain_state: chain_state::ChainState,
    message_stream: broadcast::Receiver<(ChainMessage, broadcast::Sender<ChainResponse>)>,
}

impl ChainInterfaceImpl {
    #[must_use]
    pub fn new(
        db: Box<dyn Db>,
        executor: Box<dyn TransactionExecutor>,
    ) -> (Self, messenger::Sender<ChainMessage, ChainResponse>) {
        let (tx, rx) = messenger::channel(100, Some(100));
        let chain_state = db.get_chain_state().unwrap_or_default().unwrap_or_default();
        (
            Self {
                db,
                executor,
                chain_state,
                message_stream: rx,
            },
            tx,
        )
    }
}

#[async_trait::async_trait]
impl ChainInterface for ChainInterfaceImpl {
    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError> {
        self.chain_state.insert_deposit_intent(intent.clone());
        self.db.insert_deposit_intent(intent)
    }

    fn get_account(&self, address: &str) -> Option<Account> {
        self.chain_state.get_account(address).cloned()
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        Ok(self.chain_state.get_all_deposit_intents())
    }

    fn get_deposit_intent_by_address(&self, address: &str) -> Option<DepositIntent> {
        self.chain_state
            .get_deposit_intent_by_address(address)
            .cloned()
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

    async fn add_transaction_to_block(
        &mut self,
        transaction: Transaction,
    ) -> Result<(), NodeError> {
        self.chain_state.add_transaction_to_block(transaction);
        Ok(())
    }

    fn get_proposed_block(
        &self,
        previous_block: Option<Block>,
        proposer: Vec<u8>,
    ) -> Result<Block, NodeError> {
        Ok(self
            .chain_state
            .get_proposed_block(previous_block, proposer))
    }
}

#[cfg(test)]
mod tests;
