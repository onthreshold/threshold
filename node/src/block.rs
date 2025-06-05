use bincode::{Decode, Encode};
use sha2::{Digest, Sha256};

pub trait Block: Encode + Decode<()> {
    fn get_hash(&self) -> Vec<u8>;
    fn serialize(&self) -> Vec<u8> {
        bincode::encode_to_vec(self, bincode::config::standard()).unwrap()
    }
    fn deserialize(data: &[u8]) -> Self {
        bincode::decode_from_slice(data, bincode::config::standard())
            .unwrap()
            .0
    }
}

#[derive(Encode, Decode)]
pub struct GenesisBlock {
    pub pub_key: Vec<u8>,
    pub hash: Vec<u8>,
}

#[derive(Encode, Decode)]
pub struct BlockState {
    pub previous_block_hash: String,
    pub transactions: Vec<Transaction>,
    pub hash: Vec<u8>,
}

#[derive(Encode, Decode)]
pub enum Transaction {
    Noop,
}

impl AsRef<[u8]> for Transaction {
    fn as_ref(&self) -> &[u8] {
        match self {
            Transaction::Noop => b"Noop",
        }
    }
}

impl BlockState {
    pub fn new(previous_block_hash: String, transactions: Vec<Transaction>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(previous_block_hash.as_bytes());
        transactions.iter().for_each(|t| {
            hasher.update(t.as_ref());
        });
        let hash = hasher.finalize().to_vec();

        Self {
            previous_block_hash,
            transactions,
            hash,
        }
    }
}

impl Block for BlockState {
    fn get_hash(&self) -> Vec<u8> {
        self.hash.clone()
    }
}

impl GenesisBlock {
    pub fn new(pub_key: Vec<u8>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(&pub_key);
        let hash = hasher.finalize().to_vec();

        Self { pub_key, hash }
    }
}

impl Block for GenesisBlock {
    fn get_hash(&self) -> Vec<u8> {
        self.hash.clone()
    }
}
