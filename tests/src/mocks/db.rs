use std::{collections::HashMap, sync::RwLock};

use abci::{chain_state::ChainState, db::Db};
use protocol::block::{Block, BlockHash};
use types::{errors::NodeError, intents::DepositIntent, utxo::Utxo};

pub struct MockDb {
    pub blocks: RwLock<HashMap<BlockHash, Block>>,
    pub chain_state: RwLock<ChainState>,
    pub height_map: RwLock<HashMap<u64, BlockHash>>,
    pub tip_block_hash: RwLock<Option<BlockHash>>,
    pub deposit_intents: RwLock<HashMap<String, DepositIntent>>,
    pub utxos: RwLock<HashMap<String, Utxo>>,
}

impl Default for MockDb {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDb {
    pub fn new() -> Self {
        Self {
            blocks: RwLock::new(HashMap::new()),
            chain_state: RwLock::new(ChainState::new()),
            height_map: RwLock::new(HashMap::new()),
            tip_block_hash: RwLock::new(None),
            deposit_intents: RwLock::new(HashMap::new()),
            utxos: RwLock::new(HashMap::new()),
        }
    }
}

impl Db for MockDb {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError> {
        let height_map = self.height_map.read().unwrap();
        let blocks = self.blocks.read().unwrap();
        let block_hash = height_map.get(&height).cloned();
        Ok(block_hash.and_then(|hash| blocks.get(&hash).cloned()))
    }

    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError> {
        let blocks = self.blocks.read().unwrap();
        Ok(blocks.get(&hash).cloned())
    }

    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError> {
        Ok(*self.tip_block_hash.read().unwrap())
    }

    fn insert_chain_state(&self, chain_state: ChainState) -> Result<(), NodeError> {
        *self.chain_state.write().unwrap() = chain_state;
        Ok(())
    }

    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError> {
        Ok(Some(self.chain_state.read().unwrap().clone()))
    }

    fn insert_block(&self, block: Block) -> Result<(), NodeError> {
        let mut blocks = self.blocks.write().unwrap();
        let mut height_map = self.height_map.write().unwrap();
        let mut tip_block_hash = self.tip_block_hash.write().unwrap();

        blocks.insert(block.hash(), block.clone());
        height_map.insert(block.header.height, block.hash());
        *tip_block_hash = Some(block.hash());
        Ok(())
    }

    fn insert_deposit_intent(&self, intent: DepositIntent) -> Result<(), NodeError> {
        let mut deposit_intents = self.deposit_intents.write().unwrap();
        deposit_intents.insert(intent.deposit_tracking_id.clone(), intent);
        Ok(())
    }

    fn flush_state(&self, chain_state: &ChainState) -> Result<(), NodeError> {
        *self.chain_state.write().unwrap() = chain_state.clone();
        Ok(())
    }

    fn get_deposit_intent(&self, tracking_id: &str) -> Result<Option<DepositIntent>, NodeError> {
        let deposit_intents = self.deposit_intents.read().unwrap();
        Ok(deposit_intents.get(tracking_id).cloned())
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        let deposit_intents = self.deposit_intents.read().unwrap();
        Ok(deposit_intents.values().cloned().collect())
    }

    fn get_deposit_intent_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DepositIntent>, NodeError> {
        let deposit_intents = self.deposit_intents.read().unwrap();
        Ok(deposit_intents
            .values()
            .find(|intent| intent.deposit_address == address)
            .cloned())
    }

    fn store_utxos(&self, utxos: Vec<Utxo>) -> Result<(), NodeError> {
        let mut utxos_map = self.utxos.write().unwrap();
        for utxo in utxos {
            utxos_map.insert(utxo.outpoint.txid.to_string(), utxo);
        }
        Ok(())
    }

    fn get_utxos(&self) -> Result<Vec<Utxo>, NodeError> {
        let utxos = self.utxos.read().unwrap();
        Ok(utxos.values().cloned().collect())
    }

    fn remove_deposit_intent(&self, intent: DepositIntent) -> Result<(), NodeError> {
        let mut deposit_intents = self.deposit_intents.write().unwrap();
        deposit_intents.remove(&intent.deposit_tracking_id);
        Ok(())
    }
}
