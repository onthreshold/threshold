use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct DepositIntent {
    pub amount_sat: u64,
    pub user_pubkey: String,
    pub deposit_tracking_id: String,
    pub deposit_address: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendIntent {
    pub amount_sat: u64,
    pub address_to: String,
    pub public_key: String,
    pub blocks_to_confirm: Option<u32>,
}
