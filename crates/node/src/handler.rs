use types::errors::NodeError;

use crate::{Network, NodeState, db::Db, swarm_manager::NetworkEvent};

#[async_trait::async_trait]
pub trait Handler<N: Network, D: Db>: Send {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError>;
}
