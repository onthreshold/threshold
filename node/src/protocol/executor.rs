use crate::{
    errors::NodeError,
    protocol::{
        chain_state::ChainState,
        transaction::{Operation, Transaction},
    },
};

pub struct TransactionExecutor;

impl TransactionExecutor {
    pub fn execute_transaction(
        transaction: Transaction,
        chain_state: &mut ChainState,
    ) -> Result<(), NodeError> {
        for operation in transaction.operations {
            match operation {
                Operation::OpIncrementBalance { account_id, amount } => {
                    let account = chain_state.get_account_mut(&account_id);
                    if let Some(account) = account {
                        account.balance += amount;
                    } else {
                        return Err(NodeError::Error("Account not found".to_string()));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::protocol::chain_state::Account;

    use super::*;

    #[test]
    fn test_execute_transaction() {
        let accounts = HashMap::from([(
            "1".to_string(),
            Account {
                balance: 0,
                address: "1".to_string(),
            },
        )]);
        let mut chain_state = ChainState::new_with_accounts(accounts, 0);
        let transaction = Transaction::new(vec![Operation::OpIncrementBalance {
            account_id: "1".to_string(),
            amount: 100,
        }]);
        TransactionExecutor::execute_transaction(transaction, &mut chain_state).unwrap();
        assert_eq!(chain_state.get_account("1").unwrap().balance, 100);
    }

    #[test]
    fn test_execute_transaction_with_multiple_operations() {
        let accounts = HashMap::from([
            (
                "1".to_string(),
                Account {
                    balance: 100,
                    address: "1".to_string(),
                },
            ),
            (
                "2".to_string(),
                Account {
                    balance: 200,
                    address: "2".to_string(),
                },
            ),
            (
                "3".to_string(),
                Account {
                    balance: 300,
                    address: "3".to_string(),
                },
            ),
        ]);
        let mut chain_state = ChainState::new_with_accounts(accounts, 0);
        let transaction = Transaction::new(vec![
            Operation::OpIncrementBalance {
                account_id: "1".to_string(),
                amount: 100,
            },
            Operation::OpIncrementBalance {
                account_id: "2".to_string(),
                amount: 200,
            },
            Operation::OpIncrementBalance {
                account_id: "1".to_string(),
                amount: 100,
            },
            Operation::OpIncrementBalance {
                account_id: "3".to_string(),
                amount: 300,
            },
        ]);
        TransactionExecutor::execute_transaction(transaction, &mut chain_state).unwrap();
        assert_eq!(chain_state.get_account("1").unwrap().balance, 300);
        assert_eq!(chain_state.get_account("2").unwrap().balance, 400);
        assert_eq!(chain_state.get_account("3").unwrap().balance, 600);
    }

    #[test]
    fn test_execute_transaction_with_zero_amount() {
        let accounts = HashMap::from([(
            "1".to_string(),
            Account {
                balance: 0,
                address: "1".to_string(),
            },
        )]);
        let mut chain_state = ChainState::new_with_accounts(accounts, 0);
        let transaction = Transaction::new(vec![Operation::OpIncrementBalance {
            account_id: "1".to_string(),
            amount: 0,
        }]);
        let result = TransactionExecutor::execute_transaction(transaction, &mut chain_state);
        assert!(result.is_ok());
        assert_eq!(chain_state.get_account("1").unwrap().balance, 0);
    }

    #[test]
    fn test_execute_transaction_with_invalid_account() {
        let mut chain_state = ChainState::new();
        let transaction = Transaction::new(vec![Operation::OpIncrementBalance {
            account_id: "1".to_string(),
            amount: 100,
        }]);
        let result = TransactionExecutor::execute_transaction(transaction, &mut chain_state);
        assert!(result.is_err());
    }
}
