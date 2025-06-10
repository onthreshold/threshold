use std::any::Any;

use types::errors::NodeError;

use crate::{Network, NodeState, db::Db, swarm_manager::NetworkEvent};
use protocol::oracle::Oracle;

#[async_trait::async_trait]
pub trait Handler<N: Network, D: Db, O: Oracle>: Send + Any {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, O>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError>;
}

impl<N: Network, D: Db, O: Oracle> dyn Handler<N, D, O> {
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        (self as &dyn Any).downcast_ref::<T>()
    }
}
