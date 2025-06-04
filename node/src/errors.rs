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
    SendError(SendError<NetworkMessage>),
    RecvError,
}

impl Error for NodeError {}
