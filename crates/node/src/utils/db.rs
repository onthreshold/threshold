use rocksdb::DB;

use protocol::{
    block::{Block, BlockHash},
    chain_state::ChainState,
};
use types::errors::NodeError;

use types::intents::DepositIntent;

pub trait Db: Send {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError>;
    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError>;
    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError>;
    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError>;
    fn insert_chain_state(&mut self, chain_state: ChainState) -> Result<(), NodeError>;
    fn insert_block(&mut self, block: Block) -> Result<(), NodeError>;
    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError>;
    fn get_deposit_intent(&self, tracking_id: &str) -> Result<Option<DepositIntent>, NodeError>;
    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError>;
    fn get_deposit_intent_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DepositIntent>, NodeError>;
}

pub struct RocksDb {
    pub db: DB,
}

impl RocksDb {
    pub fn new(path: &str) -> Self {
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cfs = vec!["deposit_intents", "blocks", "chain_state"];
        let db = DB::open_cf(&opts, path, cfs).unwrap();

        Self { db }
    }
}

impl Db for RocksDb {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError> {
        let height_key = format!("h:{}", height);
        let block_hash = self.db.get_cf(
            self.db.cf_handle("blocks").unwrap(),
            &height_key,
        )?;
        if let Some(block_hash) = block_hash {
            let block_key = format!("b:{}", hex::encode(&block_hash));
            let block = self.db.get_cf(
                self.db.cf_handle("blocks").unwrap(),
                &block_key,
            )?;
            Ok(block.and_then(|b| Block::deserialize(&b).ok()))
        } else {
            Ok(None)
        }
    }

    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError> {
        let block_key = format!("b:{}", hex::encode(hash));
        let block = self.db.get_cf(
            self.db.cf_handle("blocks").unwrap(),
            &block_key,
        )?;
        Ok(block.and_then(|b| Block::deserialize(&b).ok()))
    }

    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError> {
        let tip = self
            .db
            .get_cf(self.db.cf_handle("blocks").unwrap(), "h:tip")?;
        Ok(tip.and_then(|b| b.as_slice().try_into().ok()))
    }

    fn insert_chain_state(&mut self, chain_state: ChainState) -> Result<(), NodeError> {
        self.db.put_cf(
            self.db.cf_handle("chain_state").unwrap(),
            "c:state",
            chain_state.serialize()?,
        )?;
        Ok(())
    }

    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError> {
        let chain_state = self
            .db
            .get_cf(self.db.cf_handle("chain_state").unwrap(), "c:state")?;
        Ok(chain_state.and_then(|b| ChainState::deserialize(&b).ok()))
    }

    fn insert_block(&mut self, block: Block) -> Result<(), NodeError> {
        let block_hash = block.hash();
        let block_key = format!("b:{}", hex::encode(block_hash));
        let height_key = format!("h:{}", block.header.height);
        
        self.db
            .put_cf(
                self.db.cf_handle("blocks").unwrap(),
                &block_key,
                block.serialize()?,
            )
            .map_err(|e| NodeError::Error(format!("Failed to insert block: {}", e)))?;

        self.db
            .put_cf(
                self.db.cf_handle("blocks").unwrap(),
                &height_key,
                block_hash,
            )
            .map_err(|e| NodeError::Error(format!("Failed to insert block: {}", e)))?;

        self.db
            .put_cf(self.db.cf_handle("blocks").unwrap(), "h:tip", block_hash)?;

        Ok(())
    }

    fn insert_deposit_intent(&mut self, intent: DepositIntent) -> Result<(), NodeError> {
        let key_di = format!("di:{}", intent.deposit_tracking_id);
        let key_da = format!("da:{}", intent.deposit_address);

        let value = bincode::encode_to_vec(&intent, bincode::config::standard())
            .map_err(|e| NodeError::Error(format!("encode di: {}", e)))?;

        // 1) store canonical row
        self.db.put_cf(
            self.db.cf_handle("deposit_intents").unwrap(),
            key_di.as_bytes(),
            &value,
        )?;

        // 2) store address → tracking-id index
        self.db.put_cf(
            self.db.cf_handle("deposit_intents").unwrap(),
            key_da.as_bytes(),
            intent.deposit_tracking_id.as_bytes(),
        )?;
        Ok(())
    }

    fn get_deposit_intent(&self, tracking_id: &str) -> Result<Option<DepositIntent>, NodeError> {
        let key = format!("di:{}", tracking_id);
        let value = self
            .db
            .get_cf(self.db.cf_handle("deposit_intents").unwrap(), &key)?;

        Ok(value.and_then(|v| {
            bincode::decode_from_slice(&v, bincode::config::standard())
                .ok()
                .map(|(intent, _)| intent)
        }))
    }

    fn get_deposit_intent_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DepositIntent>, NodeError> {
        // Step 1: addr → tracking-id
        let key_da = format!("da:{}", address);
        let tracking_id = match self.db.get_cf(
            self.db.cf_handle("deposit_intents").unwrap(),
            key_da.as_bytes(),
        )? {
            Some(bytes) => String::from_utf8(bytes).ok(),
            None => None,
        };
        if let Some(id) = tracking_id {
            self.get_deposit_intent(&id)
        } else {
            Ok(None)
        }
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        let cf_handle = self.db.cf_handle("deposit_intents").unwrap();
        let iter = self.db.iterator_cf(cf_handle, rocksdb::IteratorMode::Start);
        let mut intents = Vec::new();

        for (_, value) in iter.flatten() {
            if let Ok((intent, _)) =
                bincode::decode_from_slice::<DepositIntent, _>(&value, bincode::config::standard())
            {
                intents.push(intent);
            }
        }
        Ok(intents)
    }
}
