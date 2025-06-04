use std::error::Error;

use derive_more::Display;

#[derive(Debug, Display)]
pub enum NodeError {
    Error(String),
}

impl Error for NodeError {}
