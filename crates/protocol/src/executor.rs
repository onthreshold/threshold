use std::collections::HashMap;

use bitcoin::{Txid, hashes::Hash};

use oracle::oracle::Oracle;
use types::errors::NodeError;

use crate::{
    chain_state::{Account, ChainState},
    transaction::{Operation, Transaction},
};

pub struct TransactionExecutor<O: Oracle> {
    pub oracle: O,
    pub allowance_list: HashMap<String, u64>,
    pub stack: Vec<Vec<u8>>,
    pub error: Option<NodeError>,
    pub new_chain_state: ChainState,
}

impl<O: Oracle> TransactionExecutor<O> {
    pub fn new(oracle: O) -> Self {
        Self {
            oracle,
            allowance_list: HashMap::new(),
            stack: Vec::new(),
            error: None,
            new_chain_state: ChainState::new(),
        }
    }

    pub async fn execute_transaction(
        &mut self,
        transaction: Transaction,
        chain_state: ChainState,
    ) -> Result<ChainState, NodeError> {
        self.new_chain_state = chain_state.clone();
        for operation in transaction.operations {
            match operation {
                Operation::OpPush { value } => {
                    self.push_to_stack(value);
                }
                Operation::OpCheckOracle => {
                    self.op_check_oracle().await?;
                }
                Operation::OpIncrementBalance => {
                    self.op_increment_balance().await?;
                }
            }
        }
        Ok(self.new_chain_state.clone())
    }

    pub fn push_to_stack(&mut self, value: Vec<u8>) {
        self.stack.push(value);
    }

    pub fn pop_from_stack(&mut self) -> Option<Vec<u8>> {
        self.stack.pop()
    }

    pub fn signal_error(&mut self, error: NodeError) -> NodeError {
        self.stack.push(0u64.to_be_bytes().to_vec());
        self.error = Some(error.clone());
        error
    }

    pub async fn op_check_oracle(&mut self) -> Result<(), NodeError> {
        let tx_hash = self
            .pop_from_stack()
            .ok_or_else(|| self.signal_error(NodeError::Error("Missing tx hash".to_string())))?;

        let tx_hash = Txid::from_slice(&tx_hash)
            .map_err(|e| self.signal_error(NodeError::Error(e.to_string())))?;

        let address = self
            .pop_from_stack()
            .ok_or_else(|| self.signal_error(NodeError::Error("Missing address".to_string())))?;

        let address = String::from_utf8(address)
            .map_err(|e| self.signal_error(NodeError::Error(e.to_string())))?;

        let amount = self
            .pop_from_stack()
            .ok_or_else(|| self.signal_error(NodeError::Error("Missing amount".to_string())))?;

        let amount = u64::from_be_bytes(
            amount
                .try_into()
                .map_err(|_| self.signal_error(NodeError::Error("Invalid amount".to_string())))?,
        );

        let verified = self
            .oracle
            .validate_transaction(&address, amount, tx_hash)
            .await
            .map_err(|e| self.signal_error(e))?;

        if verified {
            let current_allowance = self.allowance_list.get(&address).copied().unwrap_or(0);
            self.allowance_list
                .insert(address, current_allowance + amount);

            self.push_to_stack(1u64.to_be_bytes().to_vec());
        } else {
            self.push_to_stack(0u64.to_be_bytes().to_vec());
        }

        Ok(())
    }

    pub async fn op_increment_balance(&mut self) -> Result<(), NodeError> {
        let address = self
            .pop_from_stack()
            .ok_or_else(|| self.signal_error(NodeError::Error("Missing address".to_string())))?;

        let amount = self
            .pop_from_stack()
            .ok_or_else(|| self.signal_error(NodeError::Error("Missing amount".to_string())))?;

        let address = String::from_utf8(address)
            .map_err(|e| self.signal_error(NodeError::Error(e.to_string())))?;

        let amount = u64::from_be_bytes(
            amount
                .try_into()
                .map_err(|_| self.signal_error(NodeError::Error("Invalid amount".to_string())))?,
        );

        let allowed = {
            let allowance = self.allowance_list.get(&address).copied().unwrap_or(0);
            allowance >= amount
        };

        if !allowed {
            return Err(self.signal_error(NodeError::Error("Insufficient allowance".to_string())));
        }

        // Deduct from allowance
        let current_allowance = self.allowance_list.get(&address).copied().unwrap_or(0);
        self.allowance_list
            .insert(address.clone(), current_allowance - amount);

        let mut account = self
            .new_chain_state
            .get_account(&address)
            .cloned()
            .unwrap_or(Account {
                address: address.clone(),
                balance: 0,
            });

        account.balance += amount;

        self.new_chain_state.upsert_account(&address, account);

        // Push success to stack
        self.push_to_stack(1u64.to_be_bytes().to_vec());

        Ok(())
    }
}
