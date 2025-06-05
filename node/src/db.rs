use rocksdb::DB;

use crate::errors::NodeError;

pub struct Db {
    pub db: DB,
}

impl Db {
    pub fn new(path: &str) -> Self {
        let db = DB::open_default(path).unwrap();
        Self { db }
    }

    pub fn get_value(&self, key: &str) -> Option<String> {
        let value = self.db.get(key).unwrap();

        value.map(|v| String::from_utf8(v).unwrap())
    }

    pub fn set_value(&self, key: &str, value: &str) {
        self.db.put(key, value).unwrap();
    }

    pub fn insert_block(&self, hash: Vec<u8>, block: Vec<u8>) -> Result<(), NodeError> {
        self.db
            .put(format!("b:{}", hex::encode(hash)), block)
            .map_err(|e| NodeError::Error(format!("Failed to insert block: {}", e)))?;

        Ok(())
    }
}
