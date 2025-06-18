use crate::chain_state::{Account, ChainState};
use crate::executor::*;
use bitcoin::hashes::Hash;
use protocol::transaction::{Operation, Transaction, TransactionType};
use types::errors::NodeError;

// Simple oracle that always returns true for testing
#[derive(Clone)]
struct AlwaysValidOracle {}

#[async_trait::async_trait]
impl oracle::oracle::Oracle for AlwaysValidOracle {
    async fn validate_transaction(
        &self,
        _address: &str,
        _amount: u64,
        _tx_hash: bitcoin::Txid,
    ) -> Result<bool, NodeError> {
        Ok(true) // Always return true for testing
    }

    async fn get_current_fee_per_vb(&self, _priority: Option<u16>) -> Result<f64, NodeError> {
        Ok(10.0)
    }

    async fn refresh_utxos(
        &self,
        _address: bitcoin::Address,
        _number_pages: u32,
        _start_transactions: Option<bitcoin::Txid>,
        _allow_unconfirmed: bool,
    ) -> Result<Vec<types::utxo::Utxo>, NodeError> {
        Ok(vec![])
    }

    async fn broadcast_transaction(&self, _tx: &bitcoin::Transaction) -> Result<String, NodeError> {
        Ok("mock_txid".to_string())
    }

    async fn get_confirmed_transactions(
        &self,
        _addresses: Vec<bitcoin::Address>,
        _min_height: u32,
        _max_height: u32,
    ) -> Result<Vec<bitcoin::Transaction>, NodeError> {
        Ok(vec![])
    }

    async fn poll_new_transactions(&mut self, _addresses: Vec<bitcoin::Address>) {
        // No-op for testing
    }

    async fn get_latest_block_height(&self) -> Result<u32, NodeError> {
        Ok(800_000) // Mock block height
    }
}

fn create_test_executor() -> TransactionExecutorImpl {
    let oracle = AlwaysValidOracle {};
    TransactionExecutorImpl::new(Box::new(oracle))
}

#[test]
fn test_executor_new() {
    let executor = create_test_executor();
    assert_eq!(executor.allowance_list.len(), 0);
    assert_eq!(executor.stack.len(), 0);
    assert!(executor.error.is_none());
}

#[test]
fn test_push_pop_stack() {
    let mut executor = create_test_executor();

    // Test push
    let data1 = vec![1, 2, 3];
    let data2 = vec![4, 5, 6];
    executor.push_to_stack(data1.clone());
    executor.push_to_stack(data2.clone());

    // Test pop (LIFO order)
    assert_eq!(executor.pop_from_stack(), Some(data2));
    assert_eq!(executor.pop_from_stack(), Some(data1));
    assert_eq!(executor.pop_from_stack(), None);
}

#[test]
fn test_signal_error() {
    let mut executor = create_test_executor();
    let error = NodeError::Error("Test error".to_string());

    let returned_error = executor.signal_error(error.clone());

    // Error should be returned
    assert_eq!(returned_error.to_string(), error.to_string());

    // Error should be stored
    assert!(executor.error.is_some());
    assert_eq!(
        executor.error.as_ref().unwrap().to_string(),
        error.to_string()
    );

    // Zero should be pushed to stack
    assert_eq!(executor.stack.len(), 1);
    assert_eq!(executor.pop_from_stack(), Some(0u64.to_be_bytes().to_vec()));
}

#[tokio::test]
async fn test_op_check_oracle_success() {
    let mut executor = create_test_executor();

    // Push test data onto stack (in reverse order since it's LIFO)
    let tx_hash = bitcoin::Txid::all_zeros();
    let address = "test_address";
    let amount = 1000u64;

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    // Execute oracle check (MockOracle always returns true)
    let result = executor.op_check_oracle().await;
    assert!(result.is_ok());

    // Check allowance was set
    assert_eq!(executor.allowance_list.get(address), Some(&amount));

    // Check success pushed to stack
    assert_eq!(executor.pop_from_stack(), Some(1u64.to_be_bytes().to_vec()));
}

#[tokio::test]
async fn test_op_check_oracle_missing_tx_hash() {
    let mut executor = create_test_executor();

    // Don't push tx_hash to stack (push nothing)
    let result = executor.op_check_oracle().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing tx hash"));
}

#[tokio::test]
async fn test_op_check_oracle_missing_address() {
    let mut executor = create_test_executor();

    // Push only tx_hash
    let tx_hash = bitcoin::Txid::all_zeros();
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    let result = executor.op_check_oracle().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing address"));
}

#[tokio::test]
async fn test_op_check_oracle_missing_amount() {
    let mut executor = create_test_executor();

    // Push only tx_hash and address
    let tx_hash = bitcoin::Txid::all_zeros();
    let address = "test_address";

    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    let result = executor.op_check_oracle().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing amount"));
}

#[tokio::test]
async fn test_op_check_oracle_invalid_tx_hash() {
    let mut executor = create_test_executor();

    // Push invalid tx_hash (wrong size)
    let invalid_tx_hash = vec![1, 2, 3]; // Should be 32 bytes
    let address = "test_address";
    let amount = 1000u64;

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(invalid_tx_hash);

    let result = executor.op_check_oracle().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_op_check_oracle_invalid_address() {
    let mut executor = create_test_executor();

    // Push invalid UTF-8 address
    let tx_hash = bitcoin::Txid::all_zeros();
    let invalid_address = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
    let amount = 1000u64;

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(invalid_address);
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    let result = executor.op_check_oracle().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_op_check_oracle_invalid_amount() {
    let mut executor = create_test_executor();

    // Push invalid amount (wrong size)
    let tx_hash = bitcoin::Txid::all_zeros();
    let address = "test_address";
    let invalid_amount = vec![1, 2, 3]; // Should be 8 bytes

    executor.push_to_stack(invalid_amount);
    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    let result = executor.op_check_oracle().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid amount"));
}

#[test]
fn test_op_increment_balance_success() {
    let mut executor = create_test_executor();

    // Set up allowance first
    let address = "test_address".to_string();
    let amount = 1000u64;
    executor.allowance_list.insert(address.clone(), amount);

    // Push data to stack
    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_increment_balance();
    assert!(result.is_ok());

    // Check account was created with correct balance
    let account = executor.new_chain_state.get_account(&address).unwrap();
    assert_eq!(account.balance, amount);

    // Check allowance was decremented
    assert_eq!(executor.allowance_list.get(&address), Some(&0));

    // Check success pushed to stack
    assert_eq!(executor.pop_from_stack(), Some(1u64.to_be_bytes().to_vec()));
}

#[test]
fn test_op_increment_balance_existing_account() {
    let mut executor = create_test_executor();

    // Set up existing account
    let address = "test_address".to_string();
    let initial_balance = 500u64;
    let increment_amount = 300u64;

    let account = Account::new(address.clone(), initial_balance);
    executor.new_chain_state.upsert_account(&address, account);

    // Set up allowance
    executor
        .allowance_list
        .insert(address.clone(), increment_amount);

    // Push data to stack
    executor.push_to_stack(increment_amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_increment_balance();
    assert!(result.is_ok());

    // Check balance was incremented
    let account = executor.new_chain_state.get_account(&address).unwrap();
    assert_eq!(account.balance, initial_balance + increment_amount);
}

#[test]
fn test_op_increment_balance_insufficient_allowance() {
    let mut executor = create_test_executor();

    let address = "test_address";
    let amount = 1000u64;
    // Set allowance to less than amount
    executor
        .allowance_list
        .insert(address.to_string(), amount - 1);

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_increment_balance();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Insufficient allowance")
    );
}

#[test]
fn test_op_increment_balance_missing_address() {
    let mut executor = create_test_executor();

    // Push only amount, no address
    let amount = 1000u64;
    executor.push_to_stack(amount.to_be_bytes().to_vec());

    let result = executor.op_increment_balance();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing amount"));
}

#[test]
fn test_op_increment_balance_missing_amount() {
    let mut executor = create_test_executor();

    // Push nothing to stack
    let result = executor.op_increment_balance();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing address"));
}

#[test]
fn test_op_decrement_balance_success() {
    let mut executor = create_test_executor();

    // Set up account with balance
    let address = "test_address".to_string();
    let initial_balance = 1000u64;
    let decrement_amount = 300u64;

    let account = Account::new(address.clone(), initial_balance);
    executor.new_chain_state.upsert_account(&address, account);

    // Push data to stack
    executor.push_to_stack(decrement_amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_decrement_balance();
    assert!(result.is_ok());

    // Check balance was decremented
    let account = executor.new_chain_state.get_account(&address).unwrap();
    assert_eq!(account.balance, initial_balance - decrement_amount);

    // Check success pushed to stack
    assert_eq!(executor.pop_from_stack(), Some(1u64.to_be_bytes().to_vec()));
}

#[test]
fn test_op_decrement_balance_insufficient_balance() {
    let mut executor = create_test_executor();

    let address = "test_address".to_string();
    let initial_balance = 100u64;
    let decrement_amount = 200u64; // More than balance

    let account = Account::new(address.clone(), initial_balance);
    executor.new_chain_state.upsert_account(&address, account);

    executor.push_to_stack(decrement_amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_decrement_balance();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Insufficient balance")
    );
}

#[test]
fn test_op_decrement_balance_nonexistent_account() {
    let mut executor = create_test_executor();

    let address = "nonexistent_address";
    let amount = 100u64;

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_decrement_balance();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Insufficient balance")
    );
}

#[tokio::test]
async fn test_execute_transaction_push_operations() {
    let mut executor = create_test_executor();
    let initial_state = ChainState::new();

    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            Operation::OpPush {
                value: vec![1, 2, 3],
            },
            Operation::OpPush {
                value: vec![4, 5, 6],
            },
        ],
    );

    let result = executor
        .execute_transaction(transaction, initial_state)
        .await;
    assert!(result.is_ok());

    // Check stack has the pushed values
    assert_eq!(executor.pop_from_stack(), Some(vec![4, 5, 6]));
    assert_eq!(executor.pop_from_stack(), Some(vec![1, 2, 3]));
}

#[tokio::test]
async fn test_execute_transaction_deposit_flow() {
    let mut executor = create_test_executor();
    let initial_state = ChainState::new();

    let address = "deposit_address";
    let amount = 1000u64;
    let tx_hash = bitcoin::Txid::all_zeros();

    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            // Push data for oracle check
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
            // Push data for balance increment
            Operation::OpPush {
                value: amount.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: address.as_bytes().to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
    );

    let result = executor
        .execute_transaction(transaction, initial_state)
        .await;
    assert!(result.is_ok());

    let final_state = result.unwrap();
    let account = final_state.get_account(address).unwrap();
    assert_eq!(account.balance, amount);
}

#[tokio::test]
async fn test_execute_transaction_withdrawal_flow() {
    let mut executor = create_test_executor();

    // Set up initial state with account
    let mut initial_state = ChainState::new();
    let address = "withdrawal_address";
    let initial_balance = 2000u64;
    let withdrawal_amount = 500u64;

    let account = Account::new(address.to_string(), initial_balance);
    initial_state.upsert_account(address, account);

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

    let result = executor
        .execute_transaction(transaction, initial_state)
        .await;
    assert!(result.is_ok());

    let final_state = result.unwrap();
    let account = final_state.get_account(address).unwrap();
    assert_eq!(account.balance, initial_balance - withdrawal_amount);
}

#[tokio::test]
async fn test_execute_transaction_error_propagation() {
    let mut executor = create_test_executor();
    let initial_state = ChainState::new();

    // Try to increment balance without oracle check (no allowance)
    let transaction = Transaction::new(
        TransactionType::Deposit,
        vec![
            Operation::OpPush {
                value: 1000u64.to_be_bytes().to_vec(),
            },
            Operation::OpPush {
                value: b"address".to_vec(),
            },
            Operation::OpIncrementBalance,
        ],
    );

    let result = executor
        .execute_transaction(transaction, initial_state)
        .await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Insufficient allowance")
    );
}

#[test]
fn test_op_increment_balance_invalid_address() {
    let mut executor = create_test_executor();

    // Set up allowance
    let amount = 1000u64;
    executor
        .allowance_list
        .insert("address".to_string(), amount);

    // Push invalid UTF-8 address
    let invalid_address = vec![0xFF, 0xFE, 0xFD];
    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(invalid_address);

    let result = executor.op_increment_balance();
    assert!(result.is_err());
}

#[test]
fn test_op_increment_balance_invalid_amount() {
    let mut executor = create_test_executor();

    let address = "test_address";
    // Push invalid amount (wrong size)
    let invalid_amount = vec![1, 2, 3]; // Should be 8 bytes

    executor.push_to_stack(invalid_amount);
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_increment_balance();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid amount"));
}

#[test]
fn test_op_decrement_balance_invalid_address() {
    let mut executor = create_test_executor();

    let amount = 1000u64;
    let invalid_address = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(invalid_address);

    let result = executor.op_decrement_balance();
    assert!(result.is_err());
}

#[test]
fn test_op_decrement_balance_invalid_amount() {
    let mut executor = create_test_executor();

    let address = "test_address";
    let invalid_amount = vec![1, 2, 3]; // Should be 8 bytes

    executor.push_to_stack(invalid_amount);
    executor.push_to_stack(address.as_bytes().to_vec());

    let result = executor.op_decrement_balance();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid amount"));
}

#[test]
fn test_op_decrement_balance_missing_address() {
    let mut executor = create_test_executor();

    // Push only amount, no address
    let amount = 1000u64;
    executor.push_to_stack(amount.to_be_bytes().to_vec());

    let result = executor.op_decrement_balance();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing amount"));
}

#[test]
fn test_op_decrement_balance_missing_amount() {
    let mut executor = create_test_executor();

    // Push nothing to stack
    let result = executor.op_decrement_balance();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing address"));
}

#[tokio::test]
async fn test_op_check_oracle_allowance_accumulation() {
    let mut executor = create_test_executor();

    let tx_hash = bitcoin::Txid::all_zeros();
    let address = "test_address";
    let amount1 = 1000u64;
    let amount2 = 500u64;

    // First oracle check
    executor.push_to_stack(amount1.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    executor.op_check_oracle().await.unwrap();
    assert_eq!(executor.allowance_list.get(address), Some(&amount1));

    // Second oracle check should accumulate
    executor.push_to_stack(amount2.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    executor.op_check_oracle().await.unwrap();
    assert_eq!(
        executor.allowance_list.get(address),
        Some(&(amount1 + amount2))
    );
}

#[tokio::test]
async fn test_op_check_oracle_false_validation() {
    // Create oracle that returns false
    #[derive(Clone)]
    struct FalseOracle {}

    #[async_trait::async_trait]
    impl oracle::oracle::Oracle for FalseOracle {
        async fn validate_transaction(
            &self,
            _address: &str,
            _amount: u64,
            _tx_hash: bitcoin::Txid,
        ) -> Result<bool, NodeError> {
            Ok(false) // Always return false
        }

        async fn get_current_fee_per_vb(&self, _priority: Option<u16>) -> Result<f64, NodeError> {
            Ok(10.0)
        }

        async fn refresh_utxos(
            &self,
            _address: bitcoin::Address,
            _number_pages: u32,
            _start_transactions: Option<bitcoin::Txid>,
            _allow_unconfirmed: bool,
        ) -> Result<Vec<types::utxo::Utxo>, NodeError> {
            Ok(vec![])
        }

        async fn broadcast_transaction(
            &self,
            _tx: &bitcoin::Transaction,
        ) -> Result<String, NodeError> {
            Ok("mock_txid".to_string())
        }

        async fn get_confirmed_transactions(
            &self,
            _addresses: Vec<bitcoin::Address>,
            _min_height: u32,
            _max_height: u32,
        ) -> Result<Vec<bitcoin::Transaction>, NodeError> {
            Ok(vec![])
        }

        async fn poll_new_transactions(&mut self, _addresses: Vec<bitcoin::Address>) {
            // No-op for testing
        }

        async fn get_latest_block_height(&self) -> Result<u32, NodeError> {
            Ok(800_000)
        }
    }

    let oracle = FalseOracle {};
    let mut executor = TransactionExecutorImpl::new(Box::new(oracle));

    let tx_hash = bitcoin::Txid::all_zeros();
    let address = "test_address";
    let amount = 1000u64;

    executor.push_to_stack(amount.to_be_bytes().to_vec());
    executor.push_to_stack(address.as_bytes().to_vec());
    executor.push_to_stack(tx_hash.to_byte_array().to_vec());

    let result = executor.op_check_oracle().await;
    assert!(result.is_ok());

    // Should have no allowance since oracle returned false
    assert_eq!(executor.allowance_list.get(address), None);

    // Stack should have 0 (false) pushed to it
    assert_eq!(executor.pop_from_stack(), Some(0u64.to_be_bytes().to_vec()));
}
