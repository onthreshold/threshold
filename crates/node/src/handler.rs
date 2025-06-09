use std::any::Any;

use types::errors::NodeError;

use crate::{Network, NodeState, db::Db, swarm_manager::NetworkEvent};

#[async_trait::async_trait]
pub trait Handler<N: Network, D: Db>: Send + Any {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError>;
}

impl<N: Network, D: Db> dyn Handler<N, D> {
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        (self as &dyn Any).downcast_ref::<T>()
    }
}
