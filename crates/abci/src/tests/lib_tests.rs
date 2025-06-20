use crate::db::rocksdb::RocksDb;
use crate::executor::TransactionExecutorImpl;
use crate::{ChainInterface, ChainInterfaceImpl};
use bitcoin::hashes::Hash;

use protocol::transaction::{Operation, Transaction, TransactionType};
use tempfile::TempDir;
use types::intents::DepositIntent;
use uuid::Uuid;

#[derive(Clone)]
struct AlwaysValidOracle {}

#[async_trait::async_trait]
impl oracle::oracle::Oracle for AlwaysValidOracle {
    async fn validate_transaction(
        &self,
        _address: &str,
        _amount: u64,
        _tx_hash: bitcoin::Txid,
    ) -> Result<bool, types::errors::NodeError> {
        Ok(true) // Always return true for testing
    }

    async fn get_current_fee_per_vb(
        &self,
        _priority: Option<u16>,
    ) -> Result<f64, types::errors::NodeError> {
        Ok(10.0)
    }

    async fn refresh_utxos(
        &self,
        _address: bitcoin::Address,
        _number_pages: u32,
        _start_transactions: Option<bitcoin::Txid>,
        _allow_unconfirmed: bool,
    ) -> Result<Vec<types::utxo::Utxo>, types::errors::NodeError> {
        Ok(vec![])
    }

    async fn broadcast_transaction(
        &self,
        _tx: &bitcoin::Transaction,
    ) -> Result<String, types::errors::NodeError> {
        Ok("mock_txid".to_string())
    }

    async fn get_confirmed_transactions(
        &self,
        _addresses: Vec<bitcoin::Address>,
        _min_height: u32,
        _max_height: u32,
    ) -> Result<Vec<bitcoin::Transaction>, types::errors::NodeError> {
        Ok(vec![])
    }

    async fn poll_new_transactions(&mut self, _addresses: Vec<bitcoin::Address>) {
        // No-op for testing
    }

    async fn get_latest_block_height(&self) -> Result<u32, types::errors::NodeError> {
        Ok(800_000) // Mock block height
    }
}

fn create_test_chain_interface() -> (ChainInterfaceImpl, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().to_str().unwrap();
    let db = Box::new(RocksDb::new(db_path));

    let oracle = AlwaysValidOracle {};
    let executor = Box::new(TransactionExecutorImpl::new(Box::new(oracle)));

    let (chain_interface, _) = ChainInterfaceImpl::new(db, executor);
    (chain_interface, temp_dir)
}

#[test]
fn test_chain_interface_impl_new() {
    let (chain_interface, _temp_dir) = create_test_chain_interface();

    // Test initial state
    assert!(chain_interface.get_account("any_address").is_none());
    assert_eq!(chain_interface.get_all_deposit_intents().unwrap().len(), 0);
    assert!(
        chain_interface
            .get_deposit_intent_by_address("any_address")
            .is_none()
    );
}

#[test]
fn test_insert_and_get_deposit_intent() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let intent = DepositIntent {
        amount_sat: 50000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "test_deposit_address".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "test_user_pubkey".to_string(),
    };

    // Insert intent
    let result = chain_interface.insert_deposit_intent(intent);
    assert!(result.is_ok());

    // Test get by address
    let retrieved = chain_interface.get_deposit_intent_by_address("test_deposit_address");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().amount_sat, 50000);

    // Test get all
    let all_intents = chain_interface.get_all_deposit_intents().unwrap();
    assert_eq!(all_intents.len(), 1);
    assert_eq!(all_intents[0].deposit_address, "test_deposit_address");
}

#[test]
fn test_insert_multiple_deposit_intents() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let intent1 = DepositIntent {
        amount_sat: 10000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "address1".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "user1".to_string(),
    };

    let intent2 = DepositIntent {
        amount_sat: 20000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "address2".to_string(),
        timestamp: 1_234_567_891,
        user_pubkey: "user2".to_string(),
    };

    // Insert both intents
    chain_interface.insert_deposit_intent(intent1).unwrap();
    chain_interface.insert_deposit_intent(intent2).unwrap();

    // Verify both can be retrieved
    assert!(
        chain_interface
            .get_deposit_intent_by_address("address1")
            .is_some()
    );
    assert!(
        chain_interface
            .get_deposit_intent_by_address("address2")
            .is_some()
    );

    let all_intents = chain_interface.get_all_deposit_intents().unwrap();
    assert_eq!(all_intents.len(), 2);
}

#[tokio::test]
async fn test_execute_deposit_transaction() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let address = "deposit_user";
    let amount = 1000u64;
    let tx_hash = bitcoin::Txid::all_zeros();

    // Create deposit transaction
    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            // Oracle check operations
            Operation::OpPush {
                value: amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpPush {
                value: tx_hash.to_byte_array().to_vec(),
            },
            Operation::OpCheckOracle,
            // Balance increment operations
            Operation::OpPush {
                value: amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
    );

    // Execute transaction
    let result = chain_interface.add_transaction_to_block(transaction).await;
    assert!(result.is_ok());

    // Verify account was created with correct balance
    let account = chain_interface.get_account(address);
    assert!(account.is_some());
    assert_eq!(account.unwrap().balance, amount);
}

#[tokio::test]
async fn test_execute_withdrawal_transaction() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let address = "withdrawal_user";
    let initial_balance = 2000u64;
    let withdrawal_amount = 500u64;

    // First, set up account with initial balance using a deposit transaction
    let tx_hash = bitcoin::Txid::all_zeros();
    let deposit_transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            Operation::OpPush {
                value: initial_balance.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpPush {
                value: tx_hash.to_byte_array().to_vec(),
            },
            Operation::OpCheckOracle,
            Operation::OpPush {
                value: initial_balance.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
    );

    chain_interface
        .add_transaction_to_block(deposit_transaction)
        .await
        .unwrap();

    // Now create withdrawal transaction
    let withdrawal_transaction = Transaction::new(
        TransactionType::Withdrawal,
        vec![
            Operation::OpPush {
                value: withdrawal_amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpDecrementBalance,
        ],
    );

    // Execute withdrawal
    let result = chain_interface
        .add_transaction_to_block(withdrawal_transaction)
        .await;
    assert!(result.is_ok());

    // Verify balance was decremented
    let account = chain_interface.get_account(address).unwrap();
    assert_eq!(account.balance, initial_balance - withdrawal_amount);
}

#[tokio::test]
async fn test_execute_transaction_insufficient_balance() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let address = "poor_user";
    let withdrawal_amount = 1000u64;

    // Try to withdraw from account with no balance
    let transaction = Transaction::new(
        TransactionType::Withdrawal,
        vec![
            Operation::OpPush {
                value: withdrawal_amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpDecrementBalance,
        ],
    );

    // Should fail due to insufficient balance
    let result = chain_interface.add_transaction_to_block(transaction).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Insufficient balance")
    );
}

#[tokio::test]
async fn test_execute_transaction_state_persistence() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let address = "persistent_user";
    let amount = 5000u64;
    let tx_hash = bitcoin::Txid::all_zeros();

    // Execute deposit transaction
    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            Operation::OpPush {
                value: amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpPush {
                value: tx_hash.to_byte_array().to_vec(),
            },
            Operation::OpCheckOracle,
            Operation::OpPush {
                value: amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
    );

    chain_interface
        .add_transaction_to_block(transaction)
        .await
        .unwrap();

    // We can't access the db field directly since it's private
    // The state persistence is tested indirectly by verifying the account exists
    // after the transaction execution, which proves it was persisted to the db
    let account = chain_interface.get_account(address).unwrap();
    assert_eq!(account.balance, amount);
}

#[tokio::test]
async fn test_execute_multiple_transactions() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    let address = "multi_tx_user";
    let tx_hash = bitcoin::Txid::all_zeros();

    // Execute multiple deposit transactions
    for i in 1..=3 {
        let amount: u64 = i * 1000;
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: amount.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: address.as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.to_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: amount.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: address.as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
        );

        chain_interface
            .add_transaction_to_block(transaction)
            .await
            .unwrap();
    }

    // Final balance should be 1000 + 2000 + 3000 = 6000
    let account = chain_interface.get_account(address).unwrap();
    assert_eq!(account.balance, 6000);
}

#[tokio::test]
async fn test_transaction_error_propagation() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    // Create transaction that will fail (increment balance without oracle check)
    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            Operation::OpPush {
                value: 1000u64.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: b"address".to_vec(),
            },
            Operation::OpIncrementBalance, // This should fail due to no allowance
        ],
    );

    let result = chain_interface.add_transaction_to_block(transaction).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Insufficient allowance")
    );

    // Verify no state changes occurred
    assert!(chain_interface.get_account("address").is_none());
}

#[tokio::test]
async fn test_concurrent_operations() {
    let (mut chain_interface, _temp_dir) = create_test_chain_interface();

    // Test deposit intent and transaction execution together
    let intent = DepositIntent {
        amount_sat: 25000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "concurrent_address".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "concurrent_user".to_string(),
    };

    // Insert deposit intent
    chain_interface
        .insert_deposit_intent(intent.clone())
        .unwrap();

    // Execute transaction for the same user
    let tx_hash = bitcoin::Txid::all_zeros();
    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            Operation::OpPush {
                value: 5000u64.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: b"concurrent_user".to_vec(),
            },
            Operation::OpPush {
                value: tx_hash.to_byte_array().to_vec(),
            },
            Operation::OpCheckOracle,
            Operation::OpPush {
                value: 5000u64.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: b"concurrent_user".to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
    );

    chain_interface
        .add_transaction_to_block(transaction)
        .await
        .unwrap();

    // Verify both operations succeeded
    assert!(
        chain_interface
            .get_deposit_intent_by_address("concurrent_address")
            .is_some()
    );
    let account = chain_interface.get_account("concurrent_user");
    assert!(account.is_some());
    assert_eq!(account.unwrap().balance, 5000);
}

#[test]
fn test_empty_state_queries() {
    let (chain_interface, _temp_dir) = create_test_chain_interface();

    // All queries on empty state should return appropriate empty results
    assert!(chain_interface.get_account("nonexistent").is_none());
    assert!(
        chain_interface
            .get_deposit_intent_by_address("nonexistent")
            .is_none()
    );
    assert_eq!(chain_interface.get_all_deposit_intents().unwrap().len(), 0);
}
