use std::time::{SystemTime, UNIX_EPOCH};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use types::errors::NodeError;

use crate::transaction::Transaction;

pub type BlockHash = [u8; 32];
pub type StateRoot = [u8; 32];

/// Block header containing all metadata
#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHeader {
    /// Version of the block structure
    pub version: u32,

    /// Hash of the previous block
    pub previous_block_hash: BlockHash,

    /// Merkle root of the transactions in the block
    pub state_root: StateRoot,

    /// Block height/number in the chain
    pub height: u64,

    /// Proposer of this block (for PoS/PoA)
    pub proposer: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct GenesisBlock {
    pub version: u32,
    pub timestamp: u64,
    pub initial_state: GenesisState,
    pub extra_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct GenesisState {
    pub validators: Vec<ValidatorInfo>,
    pub vault_pub_key: Vec<u8>,
    pub initial_balances: Vec<(String, u64)>,
    pub chain_config: ChainConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct ValidatorInfo {
    pub pub_key: Vec<u8>,
    pub stake: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct ChainConfig {
    pub min_signers: u16,
    pub max_signers: u16,
    pub min_stake: u64,
    pub block_time_seconds: u64,
    pub max_block_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Block {
    pub header: BlockHeader,
    pub body: BlockBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct BlockBody {
    pub transactions: Vec<Transaction>,
}

impl BlockBody {
    #[must_use]
    pub const fn new(transactions: Vec<Transaction>) -> Self {
        Self { transactions }
    }
}

impl BlockBody {
    #[must_use]
    pub fn calculate_hash(&self) -> BlockHash {
        let mut hasher = Sha256::new();
        let state_bytes =
            bincode::encode_to_vec(&self.transactions, bincode::config::standard()).unwrap();
        hasher.update(&state_bytes);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }
}

impl BlockHeader {
    #[must_use]
    pub fn calculate_hash(&self) -> BlockHash {
        let mut hasher = Sha256::new();

        hasher.update(self.version.to_le_bytes());
        hasher.update(self.previous_block_hash);
        hasher.update(self.state_root);
        hasher.update(self.height.to_le_bytes());
        hasher.update(&self.proposer);

        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.hash() == other.hash()
    }
}

impl Block {
    /// Create a new block
    #[must_use]
    pub fn new(
        previous_block_hash: BlockHash,
        height: u64,
        transactions: Vec<Transaction>,
        proposer: Vec<u8>,
    ) -> Self {
        let mut hasher = Sha256::new();
        let state_bytes =
            bincode::encode_to_vec(&transactions, bincode::config::standard()).unwrap();
        hasher.update(&state_bytes);
        hasher.update(&proposer);
        hasher.update(height.to_le_bytes());
        let result = hasher.finalize();
        let mut state_root = [0u8; 32];
        state_root.copy_from_slice(&result);

        let header = BlockHeader {
            version: 1,
            previous_block_hash,
            state_root,
            height,
            proposer,
        };

        Self {
            header,
            body: BlockBody { transactions },
        }
    }

    #[must_use]
    pub const fn new_with_state_root(
        previous_block_hash: BlockHash,
        height: u64,
        state_root: StateRoot,
        transactions: Vec<Transaction>,
        proposer: Vec<u8>,
    ) -> Self {
        let header = BlockHeader {
            version: 1,
            previous_block_hash,
            state_root,
            height,
            proposer,
        };

        Self {
            header,
            body: BlockBody { transactions },
        }
    }

    #[must_use]
    pub fn hash(&self) -> BlockHash {
        let mut hasher = Sha256::new();
        hasher.update(self.header.calculate_hash());
        hasher.update(self.body.calculate_hash());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    pub fn serialize(&self) -> Result<Vec<u8>, NodeError> {
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| NodeError::Error(format!("Failed to serialize block: {e}")))
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NodeError> {
        let (block, _): (Self, _) =
            bincode::decode_from_slice(data, bincode::config::standard())
                .map_err(|e| NodeError::Error(format!("Failed to deserialize block: {e}")))?;
        Ok(block)
    }
}

impl GenesisBlock {
    /// Create a new genesis block
    #[must_use]
    pub fn new(
        validators: Vec<ValidatorInfo>,
        chain_config: ChainConfig,
        vault_pub_key: Vec<u8>,
    ) -> Self {
        Self {
            version: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            initial_state: GenesisState {
                validators,
                vault_pub_key,
                initial_balances: vec![],
                chain_config,
            },
            extra_data: b"Genesis Block".to_vec(),
        }
    }

    #[must_use]
    pub fn to_block(&self) -> Block {
        let mut hasher = Sha256::new();
        hasher.update(b"GENESIS");
        hasher.update(self.timestamp.to_le_bytes());
        let state_bytes =
            bincode::encode_to_vec(&self.initial_state, bincode::config::standard()).unwrap();
        hasher.update(&state_bytes);
        let result = hasher.finalize();
        let mut state_root = [0u8; 32];
        state_root.copy_from_slice(&result);

        Block::new(
            [0u8; 32], // No previous block
            0,         // Height 0
            vec![],    // No transactions in genesis
            self.initial_state
                .validators
                .first()
                .map(|v| v.pub_key.clone())
                .unwrap_or_default(),
        )
    }

    /// Get hash of genesis block
    #[must_use]
    pub fn hash(&self) -> BlockHash {
        self.to_block().hash()
    }
}
