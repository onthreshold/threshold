use std::any::Any;

use abci::{
    ChainInterface,
    chain_state::{Account, ChainState},
    db::Db,
    executor::TransactionExecutor,
};
use frost_secp256k1::keys::PublicKeyPackage;
use protocol::{
    block::{ChainConfig, GenesisBlock, ValidatorInfo},
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

    fn get_account(&self, address: &str) -> Option<Account> {
        self.chain_state.get_account(address).cloned()
    }
}

impl TestChainInterface for MockChainInterface {
    fn as_mock_mut(&mut self) -> Option<&mut MockChainInterface> {
        Some(self)
    }
}

// Helper function for tests to set up accounts
// Updated to work with the message-passing architecture
pub async fn setup_test_account(
    node: &mut crate::mocks::network::MockNodeState,
    address: &str,
    account: Account,
) -> Result<(), types::errors::NodeError> {
    // Since there's no direct "create account" message in the current ChainMessage enum,
    // we need to set up accounts through transactions.
    // For testing purposes, we'll create a deposit transaction to establish the account balance.

    use protocol::transaction::{Operation, Transaction, TransactionType};

    let transaction = Transaction {
        r#type: TransactionType::Deposit,
        operations: vec![
            Operation::OpPush {
                value: account.balance.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs(),
        version: 1,
    };

    match node
        .chain_interface_tx
        .send_message_with_response(abci::ChainMessage::ExecuteTransaction { transaction })
        .await
    {
        Ok(abci::ChainResponse::ExecuteTransaction { error: None }) => Ok(()),
        Ok(abci::ChainResponse::ExecuteTransaction { error: Some(e) }) => Err(e),
        _ => Err(types::errors::NodeError::Error(
            "Failed to set up test account".to_string(),
        )),
    }
}
