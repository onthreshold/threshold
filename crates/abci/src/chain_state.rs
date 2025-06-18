use std::collections::HashMap;

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use types::{errors::NodeError, intents::DepositIntent};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Account {
    pub address: String,
    pub balance: u64,
}

impl Account {
    #[must_use]
    pub const fn new(address: String, balance: u64) -> Self {
        Self { address, balance }
    }

    #[must_use]
    pub fn increment_balance(&self, amount: u64) -> Self {
        let new_balance = self.balance + amount;

        Self {
            address: self.address.clone(),
            balance: new_balance,
        }
    }

    #[must_use]
    pub fn decrement_balance(&self, amount: u64) -> Self {
        let new_balance = self.balance.saturating_sub(amount);
        Self {
            address: self.address.clone(),
            balance: new_balance,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct ChainState {
    // address -> account
    accounts: HashMap<String, Account>,
    deposit_intents: Vec<DepositIntent>,
    block_height: u64,
}

impl Default for ChainState {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: implement periodic flushing of chain state to rocksdb
impl ChainState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            deposit_intents: Vec::new(),
            block_height: 0,
        }
    }

    #[must_use]
    pub const fn new_with_accounts(accounts: HashMap<String, Account>, block_height: u64) -> Self {
        Self {
            accounts,
            deposit_intents: Vec::new(),
            block_height,
        }
    }

    #[must_use]
    pub fn get_account(&self, address: &str) -> Option<&Account> {
        self.accounts.get(address)
    }

    pub fn upsert_account(&mut self, address: &str, account: Account) {
        self.accounts.insert(address.to_string(), account);
    }

    pub fn insert_deposit_intent(&mut self, intent: DepositIntent) {
        self.deposit_intents.push(intent);
    }

    #[must_use]
    pub fn get_all_deposit_intents(&self) -> Vec<DepositIntent> {
        self.deposit_intents.clone()
    }

    #[must_use]
    pub fn get_deposit_intent_by_address(&self, address: &str) -> Option<&DepositIntent> {
        self.deposit_intents
            .iter()
            .find(|intent| intent.deposit_address == address)
    }

    #[must_use]
    pub const fn get_block_height(&self) -> u64 {
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
