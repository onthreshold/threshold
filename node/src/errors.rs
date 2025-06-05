use std::error::Error;

use derive_more::Display;
use tokio::sync::mpsc::error::SendError;

use crate::swarm_manager::NetworkMessage;

#[derive(Debug, Display)]
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

impl Error for NodeError {}
