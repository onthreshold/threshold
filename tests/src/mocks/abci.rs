use std::any::Any;

use abci::{
    ChainInterface,
    chain_state::{Account, ChainState},
    db::Db,
    executor::TransactionExecutor,
};
use frost_secp256k1::keys::PublicKeyPackage;
use protocol::{
    block::{Block, ChainConfig, GenesisBlock, ValidatorInfo},
    transaction::Transaction,
};
use types::{errors::NodeError, intents::DepositIntent};

use super::db::MockDb;

// Test helper trait for accessing mock-specific functionality
pub trait TestChainInterface {
    fn as_mock_mut(&mut self) -> Option<&mut MockChainInterface>;
}

pub struct MockTransactionExecutor;

#[async_trait::async_trait]
impl TransactionExecutor for MockTransactionExecutor {
    async fn execute_transaction(
        &mut self,
        transaction: Transaction,
        mut chain_state: ChainState,
    ) -> Result<ChainState, NodeError> {
        // Simple mock implementation of transaction execution
        for operation in &transaction.operations {
            match operation {
                protocol::transaction::Operation::OpPush { .. } => {
                    // Mock implementation - just continue
                }
                protocol::transaction::Operation::OpCheckOracle => {
                    // Mock implementation - assume oracle check passes
                }
                protocol::transaction::Operation::OpIncrementBalance => {
                    // Mock implementation - for deposits, increment balance
                    if transaction.r#type == protocol::transaction::TransactionType::Deposit {
                        // This is a simplified mock - in reality we'd pop from stack
                        // For testing purposes, we'll extract from the transaction operations
                        if let (
                            Some(protocol::transaction::Operation::OpPush {
                                value: amount_bytes,
                            }),
                            Some(protocol::transaction::Operation::OpPush { value: addr_bytes }),
                        ) = (
                            transaction.operations.first(),
                            transaction.operations.get(1),
                        ) {
                            let amount = u64::from_be_bytes([
                                amount_bytes[0],
                                amount_bytes[1],
                                amount_bytes[2],
                                amount_bytes[3],
                                amount_bytes[4],
                                amount_bytes[5],
                                amount_bytes[6],
                                amount_bytes[7],
                            ]);
                            let address = String::from_utf8(addr_bytes.clone()).unwrap_or_default();

                            let current_balance = chain_state
                                .get_account(&address)
                                .map(|acc| acc.balance)
                                .unwrap_or(0);

                            chain_state.upsert_account(
                                &address,
                                Account::new(address.clone(), current_balance + amount),
                            );
                        }
                    }
                }
                protocol::transaction::Operation::OpDecrementBalance => {
                    // Mock implementation - for withdrawals, decrement balance
                    if transaction.r#type == protocol::transaction::TransactionType::Withdrawal {
                        if let (
                            Some(protocol::transaction::Operation::OpPush {
                                value: amount_bytes,
                            }),
                            Some(protocol::transaction::Operation::OpPush { value: addr_bytes }),
                        ) = (
                            transaction.operations.first(),
                            transaction.operations.get(1),
                        ) {
                            let amount = u64::from_be_bytes([
                                amount_bytes[0],
                                amount_bytes[1],
                                amount_bytes[2],
                                amount_bytes[3],
                                amount_bytes[4],
                                amount_bytes[5],
                                amount_bytes[6],
                                amount_bytes[7],
                            ]);
                            let address = String::from_utf8(addr_bytes.clone()).unwrap_or_default();

                            let current_balance = chain_state
                                .get_account(&address)
                                .map(|acc| acc.balance)
                                .unwrap_or(0);

                            if current_balance >= amount {
                                chain_state.upsert_account(
                                    &address,
                                    Account::new(address.clone(), current_balance - amount),
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(chain_state)
    }
}

pub struct MockChainInterface {
    pub db: MockDb,
    pub executor: MockTransactionExecutor,
    pub chain_state: ChainState,
}

impl MockChainInterface {
    pub fn new() -> Self {
        Self {
            db: MockDb::new(),
            executor: MockTransactionExecutor,
            chain_state: ChainState::new(),
        }
    }

    // Test helper method to set up accounts
    pub fn upsert_account(&mut self, address: &str, account: Account) {
        self.chain_state.upsert_account(address, account);
        // Also sync to db for consistency
        let _ = self.db.flush_state(&self.chain_state);
    }

    // Test helper method to access the database
    pub fn get_db(&self) -> &MockDb {
        &self.db
    }

    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Default for MockChainInterface {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ChainInterface for MockChainInterface {
    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError> {
        // Insert into both chain_state and db for consistency
        self.chain_state.insert_deposit_intent(intent.clone());
        self.db.insert_deposit_intent(intent)
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        Ok(self.chain_state.get_all_deposit_intents())
    }

    fn get_deposit_intent_by_address(&self, address: &str) -> Option<DepositIntent> {
        self.chain_state
            .get_deposit_intent_by_address(address)
            .cloned()
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
        let new_chain_state = self
            .executor
            .execute_transaction(transaction, self.chain_state.clone())
            .await?;

        self.db.flush_state(&new_chain_state)?;
        self.chain_state = new_chain_state;
        Ok(())
    }

    fn get_account(&self, address: &str) -> Option<Account> {
        self.chain_state.get_account(address).cloned()
    }

    async fn finalize_and_store_block(&mut self, block: Block) -> Result<(), NodeError> {
        // Execute all transactions in the block
        let mut new_chain_state = self.chain_state.clone();
        for transaction in &block.body.transactions {
            new_chain_state = self
                .executor
                .execute_transaction(transaction.clone(), new_chain_state)
                .await?;
        }

        // Store the block in the database
        self.db.insert_block(block.clone())?;

        // Update chain state
        self.db.flush_state(&new_chain_state)?;
        self.chain_state = new_chain_state;

        // Clear pending transactions
        self.chain_state.clear_pending_transactions();

        Ok(())
    }

    fn get_pending_transactions(&self) -> Vec<Transaction> {
        self.chain_state.get_pending_transactions().to_vec()
    }

    fn get_chain_state(&self) -> ChainState {
        self.chain_state.clone()
    }

    fn remove_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError> {
        self.chain_state.remove_deposit_intent(&intent);
        self.db.remove_deposit_intent(intent)?;
        Ok(())
    }
}

impl TestChainInterface for MockChainInterface {
    fn as_mock_mut(&mut self) -> Option<&mut MockChainInterface> {
        Some(self)
    }
}
