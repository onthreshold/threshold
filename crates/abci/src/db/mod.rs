use types::{errors::NodeError, intents::DepositIntent};

use protocol::block::{Block, BlockHash};

use crate::chain_state::ChainState;
pub mod rocksdb;

pub trait Db: Send + Sync {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError>;
    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError>;
    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError>;
    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError>;
    fn insert_chain_state(&self, chain_state: ChainState) -> Result<(), NodeError>;
    fn insert_block(&self, block: Block) -> Result<(), NodeError>;
    fn insert_deposit_intent(&self, intent: DepositIntent) -> Result<(), NodeError>;
    fn get_deposit_intent(&self, tracking_id: &str) -> Result<Option<DepositIntent>, NodeError>;
    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError>;
    fn get_deposit_intent_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DepositIntent>, NodeError>;
    fn flush_state(&self, chain_state: &ChainState) -> Result<(), NodeError>;
}
