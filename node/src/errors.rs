use std::error::Error;

use derive_more::Display;
use libp2p::gossipsub::SubscriptionError;

#[derive(Debug, Display)]
pub enum NodeError {
    Error(String),
}

impl Error for NodeError {}
