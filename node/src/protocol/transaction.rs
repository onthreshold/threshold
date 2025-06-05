use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub type TransactionId = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq)]
pub struct Transaction {
    pub version: u32,
    pub timestamp: u64,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq)]
pub enum Operation {
    OpIncrementBalance { account_id: String, amount: u64 },
}

impl Transaction {
    pub fn new(operations: Vec<Operation>) -> Self {
        Transaction {
            version: 1,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            operations,
        }
    }

    pub fn id(&self) -> TransactionId {
        let mut hasher = Sha256::new();
        hasher.update(self.version.to_le_bytes());
        hasher.update(self.timestamp.to_le_bytes());

        for op in &self.operations {
            let op_bytes = bincode::encode_to_vec(op, bincode::config::standard()).unwrap();
            hasher.update(&op_bytes);
        }

        let result = hasher.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&result);
        id
    }

    pub fn total_value(&self) -> u64 {
        self.operations
            .iter()
            .map(|op| match op {
                Operation::OpIncrementBalance { amount, .. } => *amount,
            })
            .sum()
    }
}
