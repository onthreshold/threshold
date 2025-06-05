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
        chain_state: ChainState,
    ) -> Result<ChainState, NodeError> {
        let mut addresses = Vec::new();
        let mut amounts = Vec::new();
        let mut new_chain_state = chain_state.clone();
        for operation in transaction.operations {
            match operation {
                Operation::OpPushAddress { address } => {
                    addresses.push(address);
                }
                Operation::OpPushAmount { amount } => {
                    amounts.push(amount);
                }
                Operation::OpIncrementBalance => {
                    let address = addresses
                        .pop()
                        .ok_or(NodeError::Error("Invalid transaction".to_string()))?;
                    let amount = amounts
                        .pop()
                        .ok_or(NodeError::Error("Invalid transaction".to_string()))?;
                    let account = new_chain_state
                        .get_account_mut(&address)
                        .ok_or(NodeError::Error("Account not found".to_string()))?;
                    let new_balance = account.balance + amount;
                    account.balance = new_balance;
                }
            }
        }
        Ok(new_chain_state)
    }
}
