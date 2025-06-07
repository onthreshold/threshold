use types::errors::NodeError;

use crate::{Network, swarm_manager::HandlerMessage};

#[async_trait::async_trait]
pub trait Handler<N: Network>: Send {
    async fn handle(
        &mut self,
        message: Option<HandlerMessage>,
        network_handle: &N,
    ) -> Result<(), NodeError>;
}
