use std::error::Error;

use derive_more::Display;

#[derive(Debug, Display, Clone)]
pub enum NodeError {
    Error(String),
}

#[derive(Debug)]
pub enum NetworkError {
    SendError(String),
    RecvError,
}

impl From<rocksdb::Error> for NodeError {
    fn from(e: rocksdb::Error) -> Self {
        Self::Error(e.to_string())
    }
}

impl Error for NodeError {}
