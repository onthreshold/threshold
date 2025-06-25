#[cfg(test)]
mod deposit_test {
    use bitcoin::{Txid, hashes::Hash};
    use std::collections::HashMap;
    use tokio::sync::broadcast;
    use types::network::network_event::NetworkEvent;

    use abci::{
        chain_state::{Account, ChainState},
        executor::{TransactionExecutor, TransactionExecutorImpl},
    };
    use oracle::mock::MockOracle;
    use protocol::transaction::{Operation, Transaction, TransactionType};

    fn get_test_chain_state() -> ChainState {
        let accounts = HashMap::from([
            (
                "1".to_string(),
                Account {
                    balance: 0,
                    address: "1".to_string(),
                },
            ),
            (
                "2".to_string(),
                Account {
                    balance: 100,
                    address: "2".to_string(),
                },
            ),
            (
                "3".to_string(),
                Account {
                    balance: 200,
                    address: "3".to_string(),
                },
            ),
        ]);
        ChainState::new_with_accounts(accounts, 0)
    }

    fn create_test_tx_hash() -> Txid {
        Txid::from_slice(&[1u8; 32]).unwrap()
    }

    fn create_mock_oracle(transactions: Vec<(Txid, String, u64, bool)>) -> MockOracle {
        let (tx_channel, _) = broadcast::channel::<NetworkEvent>(100);
        let mut mock_oracle = MockOracle::new(tx_channel, None);
        for (tx_hash, address, amount, is_valid) in transactions {
            mock_oracle.add_transaction(tx_hash, address, amount, is_valid);
        }
        mock_oracle
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction() {
        let accounts = HashMap::from([(
            "1".to_string(),
            Account {
                balance: 0,
                address: "1".to_string(),
            },
        )]);
        let chain_state = ChainState::new_with_accounts(accounts, 0);

        // Setup mock oracle
        let tx_hash = create_test_tx_hash();
        let mock_oracle = create_mock_oracle(vec![(tx_hash, "1".to_string(), 100, true)]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                // First, validate the transaction with oracle
                // OpCheckOracle pops: tx_hash, address, amount
                // So we push in reverse: amount, address, tx_hash
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                // Then increment the balance
                // OpIncrementBalance pops: address, amount
                // So we push in reverse: amount, address
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result_state = executor
            .execute_transaction(transaction, chain_state)
            .await
            .unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 100);
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction_with_multiple_deposits() {
        let chain_state = get_test_chain_state();

        // Setup mock oracle with multiple transactions
        let tx_hash1 = Txid::from_slice(&[1u8; 32]).unwrap();
        let tx_hash2 = Txid::from_slice(&[2u8; 32]).unwrap();
        let tx_hash3 = Txid::from_slice(&[3u8; 32]).unwrap();
        let tx_hash4 = Txid::from_slice(&[4u8; 32]).unwrap();

        let mock_oracle = create_mock_oracle(vec![
            (tx_hash1, "1".to_string(), 100, true),
            (tx_hash2, "2".to_string(), 200, true),
            (tx_hash3, "1".to_string(), 100, true),
            (tx_hash4, "3".to_string(), 300, true),
        ]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                // First deposit to account 1
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash1.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
                // Second deposit to account 2
                Operation::OpPush {
                    value: 200u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "2".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash2.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 200u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "2".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
                // Third deposit to account 1 again
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash3.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
                // Fourth deposit to account 3
                Operation::OpPush {
                    value: 300u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "3".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash4.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 300u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "3".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result_state = executor
            .execute_transaction(transaction, chain_state)
            .await
            .unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 200);
        assert_eq!(result_state.get_account("2").unwrap().balance, 300);
        assert_eq!(result_state.get_account("3").unwrap().balance, 500);
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction_with_zero_amount() {
        let chain_state = get_test_chain_state();

        let tx_hash = create_test_tx_hash();
        let mock_oracle = create_mock_oracle(vec![(tx_hash, "1".to_string(), 0, true)]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: 0u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 0u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result = executor.execute_transaction(transaction, chain_state).await;
        assert!(result.is_ok());
        let result_state = result.unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 0);
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction_with_invalid_account() {
        let chain_state = ChainState::new();

        let tx_hash = create_test_tx_hash();
        let mock_oracle = create_mock_oracle(vec![(tx_hash, "1".to_string(), 100, true)]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result = executor.execute_transaction(transaction, chain_state).await;
        // Should succeed - account will be created if it doesn't exist
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get_account("1").unwrap().balance, 100);
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction_oracle_validation_fails() {
        let chain_state = get_test_chain_state();

        let tx_hash = create_test_tx_hash();
        // Set validation to fail
        let mock_oracle = create_mock_oracle(vec![(tx_hash, "1".to_string(), 100, false)]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result = executor.execute_transaction(transaction, chain_state).await;
        // Should fail because allowance won't be granted
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction_wrong_amount_in_oracle() {
        let chain_state = get_test_chain_state();

        let tx_hash = create_test_tx_hash();
        // Oracle expects 200 but we'll try to validate 100
        let mock_oracle = create_mock_oracle(vec![(tx_hash, "1".to_string(), 200, true)]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result = executor.execute_transaction(transaction, chain_state).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_deposit_transaction_without_oracle_check() {
        let chain_state = get_test_chain_state();

        // Empty oracle since we're not checking any transactions
        let mock_oracle = create_mock_oracle(vec![]);

        // Try to increment balance without checking oracle first
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result = executor.execute_transaction(transaction, chain_state).await;
        // Should fail due to insufficient allowance
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_partial_allowance_spending() {
        let chain_state = get_test_chain_state();

        let tx_hash = create_test_tx_hash();
        // Oracle validates 100
        let mock_oracle = create_mock_oracle(vec![(tx_hash, "1".to_string(), 100, true)]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                // Validate 100
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                // Try to spend 50
                Operation::OpPush {
                    value: 50u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
                // Try to spend another 60 (should fail)
                Operation::OpPush {
                    value: 60u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result = executor.execute_transaction(transaction, chain_state).await;
        // Should fail on the second increment
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multiple_oracle_validations_same_account() {
        let chain_state = get_test_chain_state();

        let tx_hash1 = Txid::from_slice(&[1u8; 32]).unwrap();
        let tx_hash2 = Txid::from_slice(&[2u8; 32]).unwrap();

        let mock_oracle = create_mock_oracle(vec![
            (tx_hash1, "1".to_string(), 100, true),
            (tx_hash2, "1".to_string(), 50, true),
        ]);

        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                // First validation
                Operation::OpPush {
                    value: 100u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash1.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                // Second validation
                Operation::OpPush {
                    value: 50u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx_hash2.as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                // Now we should have 150 allowance, spend it all
                Operation::OpPush {
                    value: 150u64.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: "1".as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            None,
        );

        let mut executor = TransactionExecutorImpl::new(Box::new(mock_oracle));
        let result_state = executor
            .execute_transaction(transaction, chain_state)
            .await
            .unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 150);
    }
}
