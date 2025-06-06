use std::error::Error;

use derive_more::Display;
use tokio::sync::mpsc::error::SendError;

use crate::swarm_manager::NetworkMessage;

#[derive(Debug, Display, Clone)]
pub enum NodeError {
    Error(String),
}

#[derive(Debug)]
pub enum NetworkError {
    SendError(String),
    RecvError,
}

impl From<SendError<NetworkMessage>> for NetworkError {
    fn from(e: SendError<NetworkMessage>) -> Self {
        NetworkError::SendError(e.to_string())
    }
}

impl From<rocksdb::Error> for NodeError {
    fn from(e: rocksdb::Error) -> Self {
        NodeError::Error(e.to_string())
    }
}

impl Error for NodeError {}
