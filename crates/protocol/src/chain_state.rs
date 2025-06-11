use std::collections::HashMap;

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use types::errors::NodeError;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Account {
    pub address: String,
    pub balance: u64,
}

impl Account {
    pub fn new(address: String, balance: u64) -> Self {
        Self { address, balance }
    }

    pub fn update_balance(&self, amount: u64) -> Self {
        Self {
            address: self.address.clone(),
            balance: self.balance + amount,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct ChainState {
    // address -> account
    accounts: HashMap<String, Account>,
    block_height: u64,
}

impl Default for ChainState {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: implement periodic flushing of chain state to rocksdb
impl ChainState {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            block_height: 0,
        }
    }

    pub fn new_with_accounts(accounts: HashMap<String, Account>, block_height: u64) -> Self {
        Self {
            accounts,
            block_height,
        }
    }

    pub fn get_account(&self, address: &str) -> Option<&Account> {
        self.accounts.get(address)
    }

    pub fn upsert_account(&mut self, address: &str, account: Account) {
        self.accounts.insert(address.to_string(), account);
    }

    pub fn get_block_height(&self) -> u64 {
        self.block_height
    }

    pub fn serialize(&self) -> Result<Vec<u8>, NodeError> {
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| NodeError::Error(e.to_string()))
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NodeError> {
        let (chain_state, _): (Self, _) =
            bincode::decode_from_slice(data, bincode::config::standard())
                .map_err(|e| NodeError::Error(e.to_string()))?;
        Ok(chain_state)
    }
}
