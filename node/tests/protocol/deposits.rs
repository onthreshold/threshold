#[cfg(test)]
mod deposit_test {
    use std::collections::HashMap;

    use node::protocol::{
        chain_state::{Account, ChainState},
        executor::TransactionExecutor,
        transaction::{Operation, Transaction, TransactionType},
    };

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

    #[test]
    fn test_execute_deposit_transaction() {
        let accounts = HashMap::from([(
            "1".to_string(),
            Account {
                balance: 0,
                address: "1".to_string(),
            },
        )]);
        let chain_state = ChainState::new_with_accounts(accounts, 0);
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 100 },
                Operation::OpIncrementBalance,
            ],
        );
        let result_state =
            TransactionExecutor::execute_transaction(transaction, chain_state).unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 100);
    }

    #[test]
    fn test_execute_deposit_transaction_with_multiple_operations() {
        let chain_state = get_test_chain_state();
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 100 },
                Operation::OpIncrementBalance,
                Operation::OpPushAddress {
                    address: "2".to_string(),
                },
                Operation::OpPushAmount { amount: 200 },
                Operation::OpIncrementBalance,
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 100 },
                Operation::OpIncrementBalance,
                Operation::OpPushAddress {
                    address: "3".to_string(),
                },
                Operation::OpPushAmount { amount: 300 },
                Operation::OpIncrementBalance,
            ],
        );
        let result_state =
            TransactionExecutor::execute_transaction(transaction, chain_state).unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 200);
        assert_eq!(result_state.get_account("2").unwrap().balance, 300);
        assert_eq!(result_state.get_account("3").unwrap().balance, 500);
    }

    #[test]
    fn test_execute_deposit_transaction_with_zero_amount() {
        let chain_state = get_test_chain_state();
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 0 },
                Operation::OpIncrementBalance,
            ],
        );
        let result = TransactionExecutor::execute_transaction(transaction, chain_state);
        assert!(result.is_ok());
        let result_state = result.unwrap();
        assert_eq!(result_state.get_account("1").unwrap().balance, 0);
    }

    #[test]
    fn test_execute_deposit_transaction_with_invalid_account() {
        let chain_state = ChainState::new();
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 100 },
                Operation::OpIncrementBalance,
            ],
        );
        let result = TransactionExecutor::execute_transaction(transaction, chain_state);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_deposit_transaction_missing_address() {
        let chain_state = get_test_chain_state();
        // Push amount but no address
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAmount { amount: 100 },
                Operation::OpIncrementBalance,
            ],
        );
        let result = TransactionExecutor::execute_transaction(transaction, chain_state);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_deposit_transaction_missing_amount() {
        let chain_state = get_test_chain_state();
        // Push address but no amount
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpIncrementBalance,
            ],
        );
        let result = TransactionExecutor::execute_transaction(transaction, chain_state);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_deposit_transaction_multiple_pushes_single_increment() {
        let chain_state = get_test_chain_state();
        // Push multiple values but only increment once (should use last pushed values)
        let transaction = Transaction::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 100 },
                Operation::OpPushAddress {
                    address: "1".to_string(),
                },
                Operation::OpPushAmount { amount: 200 },
                Operation::OpIncrementBalance,
            ],
        );
        let result_state =
            TransactionExecutor::execute_transaction(transaction, chain_state).unwrap();
        // Should increment by 200 (the last pushed amount)
        assert_eq!(result_state.get_account("1").unwrap().balance, 200);
    }
}
