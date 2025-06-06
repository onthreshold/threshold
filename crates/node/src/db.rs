use rocksdb::DB;

use protocol::{
    block::{Block, BlockHash},
    chain_state::ChainState,
};
use types::errors::NodeError;

pub trait Db {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError>;
    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError>;
    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError>;
    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError>;
    fn insert_chain_state(&mut self, chain_state: ChainState) -> Result<(), NodeError>;
    fn insert_block(&mut self, block: Block) -> Result<(), NodeError>;
}

pub struct RocksDb {
    pub db: DB,
}

impl RocksDb {
    pub fn new(path: &str) -> Self {
        let db = DB::open_default(path).unwrap();
        Self { db }
    }
}

impl Db for RocksDb {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError> {
        let block_hash = self.db.get(format!("h:{}", height))?;
        if let Some(block_hash) = block_hash {
            let block = self.db.get(format!("b:{}", hex::encode(block_hash)))?;
            Ok(block.and_then(|b| Block::deserialize(&b).ok()))
        } else {
            Ok(None)
        }
    }

    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError> {
        let block = self.db.get(format!("b:{}", hex::encode(hash)))?;
        Ok(block.and_then(|b| Block::deserialize(&b).ok()))
    }

    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError> {
        let tip = self.db.get("h:tip")?;
        Ok(tip.and_then(|b| b.as_slice().try_into().ok()))
    }

    fn insert_chain_state(&mut self, chain_state: ChainState) -> Result<(), NodeError> {
        self.db.put("c:state", chain_state.serialize()?)?;
        Ok(())
    }

    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError> {
        let chain_state = self.db.get("c:state")?;
        Ok(chain_state.and_then(|b| ChainState::deserialize(&b).ok()))
    }

    fn insert_block(&mut self, block: Block) -> Result<(), NodeError> {
        let block_hash = block.hash();
        self.db
            .put(format!("b:{}", hex::encode(block_hash)), block.serialize()?)
            .map_err(|e| NodeError::Error(format!("Failed to insert block: {}", e)))?;

        self.db
            .put(format!("h:{}", block.header.height), block_hash)
            .map_err(|e| NodeError::Error(format!("Failed to insert block: {}", e)))?;

        self.db.put("h:tip", block_hash)?;

        Ok(())
    }
}
