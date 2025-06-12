use std::collections::HashMap;

use node::db::Db;
use node::handlers::deposit::DepositIntent;
use protocol::{
    block::{Block, BlockHash},
    chain_state::ChainState,
};
use types::errors::NodeError;

pub struct MockDb {
    pub blocks: HashMap<BlockHash, Block>,
    pub chain_state: ChainState,
    pub height_map: HashMap<u64, BlockHash>,
    pub tip_block_hash: Option<BlockHash>,
    pub deposit_intents: HashMap<String, DepositIntent>,
}

impl Default for MockDb {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDb {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            chain_state: ChainState::new(),
            height_map: HashMap::new(),
            tip_block_hash: None,
            deposit_intents: HashMap::new(),
        }
    }
}

impl Db for MockDb {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError> {
        let block_hash = self.height_map.get(&height).cloned();
        Ok(block_hash.and_then(|hash| self.blocks.get(&hash).cloned()))
    }

    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError> {
        Ok(self.blocks.get(&hash).cloned())
    }

    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError> {
        Ok(self.tip_block_hash)
    }

    fn insert_chain_state(&mut self, chain_state: ChainState) -> Result<(), NodeError> {
        self.chain_state = chain_state;
        Ok(())
    }

    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError> {
        Ok(Some(self.chain_state.clone()))
    }

    fn insert_block(&mut self, block: Block) -> Result<(), NodeError> {
        self.blocks.insert(block.hash(), block.clone());
        self.height_map.insert(block.header.height, block.hash());
        self.tip_block_hash = Some(block.hash());
        Ok(())
    }

    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError> {
        self.deposit_intents
            .insert(intent.deposit_tracking_id.clone(), intent);
        Ok(())
    }

    fn get_deposit_intent(&self, tracking_id: &str) -> Result<Option<DepositIntent>, NodeError> {
        Ok(self.deposit_intents.get(tracking_id).cloned())
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        Ok(self.deposit_intents.values().cloned().collect())
    }

    fn get_deposit_intent_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DepositIntent>, NodeError> {
        Ok(self
            .deposit_intents
            .values()
            .find(|intent| intent.deposit_address == address)
            .cloned())
    }
}
