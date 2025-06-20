use rocksdb::DB;
use std::sync::Arc;

use crate::chain_state::ChainState;
use crate::db::Db;
use protocol::block::{Block, BlockHash};
use types::intents::DepositIntent;
use types::{errors::NodeError, utxo::Utxo};

#[derive(Clone)]
pub struct RocksDb {
    pub db: Arc<DB>,
}

impl RocksDb {
    #[must_use]
    pub fn new(path: &str) -> Self {
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cfs = vec!["deposit_intents", "blocks", "chain_state", "utxos"];
        let db = Arc::new(DB::open_cf(&opts, path, cfs).unwrap());

        Self { db }
    }
}

impl Db for RocksDb {
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, NodeError> {
        let block_hash = self
            .db
            .get_cf(self.db.cf_handle("blocks").unwrap(), format!("h:{height}"))?;
        if let Some(block_hash) = block_hash {
            let block = self.db.get_cf(
                self.db.cf_handle("blocks").unwrap(),
                format!("b:{}", hex::encode(block_hash)),
            )?;
            Ok(block.and_then(|b| Block::deserialize(&b).ok()))
        } else {
            Ok(None)
        }
    }

    fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, NodeError> {
        let block = self.db.get_cf(
            self.db.cf_handle("blocks").unwrap(),
            format!("b:{}", hex::encode(hash)),
        )?;
        Ok(block.and_then(|b| Block::deserialize(&b).ok()))
    }

    fn get_tip_block_hash(&self) -> Result<Option<BlockHash>, NodeError> {
        let hash = self
            .db
            .get_cf(self.db.cf_handle("blocks").unwrap(), "tip")?;
        hash.map_or(Ok(None), |hash| {
            let mut block_hash = [0u8; 32];
            block_hash.copy_from_slice(&hash);
            Ok(Some(block_hash))
        })
    }

    fn get_chain_state(&self) -> Result<Option<ChainState>, NodeError> {
        let state = self
            .db
            .get_cf(self.db.cf_handle("chain_state").unwrap(), "current")?;
        if let Some(state) = state {
            let chain_state = ChainState::deserialize(&state)?;
            Ok(Some(chain_state))
        } else {
            Ok(Some(ChainState::new())) // Return empty state if none exists
        }
    }

    fn insert_chain_state(&self, chain_state: ChainState) -> Result<(), NodeError> {
        let serialized = chain_state.serialize()?;
        self.db.put_cf(
            self.db.cf_handle("chain_state").unwrap(),
            "current",
            serialized,
        )?;
        Ok(())
    }

    fn insert_block(&self, block: Block) -> Result<(), NodeError> {
        let block_hash = block.hash();
        let serialized = block
            .serialize()
            .map_err(|e| NodeError::Error(e.to_string()))?;

        // Store block by hash
        self.db.put_cf(
            self.db.cf_handle("blocks").unwrap(),
            format!("b:{}", hex::encode(block_hash)),
            &serialized,
        )?;

        // Store height to hash mapping
        self.db.put_cf(
            self.db.cf_handle("blocks").unwrap(),
            format!("h:{}", block.header.height),
            block_hash,
        )?;

        // Update tip
        self.db
            .put_cf(self.db.cf_handle("blocks").unwrap(), "tip", block_hash)?;

        Ok(())
    }

    fn insert_deposit_intent(&self, intent: DepositIntent) -> Result<(), NodeError> {
        let serialized = bincode::encode_to_vec(&intent, bincode::config::standard())
            .map_err(|e| NodeError::Error(e.to_string()))?;

        // Store by tracking ID
        self.db.put_cf(
            self.db.cf_handle("deposit_intents").unwrap(),
            &intent.deposit_tracking_id,
            &serialized,
        )?;

        // Store by address for quick lookup
        self.db.put_cf(
            self.db.cf_handle("deposit_intents").unwrap(),
            format!("addr:{}", intent.deposit_address),
            &serialized,
        )?;

        Ok(())
    }

    fn get_deposit_intent(&self, tracking_id: &str) -> Result<Option<DepositIntent>, NodeError> {
        let intent = self
            .db
            .get_cf(self.db.cf_handle("deposit_intents").unwrap(), tracking_id)?;

        if let Some(intent) = intent {
            let (intent, _): (DepositIntent, _) =
                bincode::decode_from_slice(&intent, bincode::config::standard())
                    .map_err(|e| NodeError::Error(e.to_string()))?;
            Ok(Some(intent))
        } else {
            Ok(None)
        }
    }

    fn get_all_deposit_intents(&self) -> Result<Vec<DepositIntent>, NodeError> {
        let cf = self.db.cf_handle("deposit_intents").unwrap();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        let mut intents = Vec::new();

        for item in iter {
            let (key, value) = item?;
            // Skip address-based keys (they start with "addr:")
            if key.starts_with(b"addr:") {
                continue;
            }

            let (intent, _): (DepositIntent, _) =
                bincode::decode_from_slice(&value, bincode::config::standard())
                    .map_err(|e| NodeError::Error(e.to_string()))?;
            intents.push(intent);
        }

        Ok(intents)
    }

    fn get_deposit_intent_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DepositIntent>, NodeError> {
        let intent = self.db.get_cf(
            self.db.cf_handle("deposit_intents").unwrap(),
            format!("addr:{address}"),
        )?;

        if let Some(intent) = intent {
            let (intent, _): (DepositIntent, _) =
                bincode::decode_from_slice(&intent, bincode::config::standard())
                    .map_err(|e| NodeError::Error(e.to_string()))?;
            Ok(Some(intent))
        } else {
            Ok(None)
        }
    }

    fn flush_state(&self, chain_state: &ChainState) -> Result<(), NodeError> {
        self.insert_chain_state(chain_state.clone())
    }

    fn store_utxos(&self, utxos: Vec<Utxo>) -> Result<(), NodeError> {
        for utxo in utxos {
            let serialized = bincode::encode_to_vec(&utxo, bincode::config::standard())
                .map_err(|e| NodeError::Error(e.to_string()))?;
            self.db.put_cf(
                self.db.cf_handle("utxos").unwrap(),
                format!("utxo:{}", utxo.outpoint.txid),
                &serialized,
            )?;
        }

        Ok(())
    }

    fn get_utxos(&self) -> Result<Vec<Utxo>, NodeError> {
        let cf = self.db.cf_handle("utxos").unwrap();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        let mut utxos = Vec::new();

        for item in iter {
            let (key, value) = item?;
            if !key.starts_with(b"utxo:") {
                continue;
            }
            let (utxo, _): (Utxo, _) =
                bincode::decode_from_slice(&value, bincode::config::standard())
                    .map_err(|e| NodeError::Error(e.to_string()))?;
            utxos.push(utxo);
        }

        Ok(utxos)
    }
}
