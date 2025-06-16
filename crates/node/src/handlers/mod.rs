pub mod balance;
pub mod deposit;
pub mod dkg;
pub mod signing;
pub mod withdrawl;

use std::any::Any;

use types::errors::NodeError;

use crate::wallet::Wallet;
use crate::{Network, NodeState, db::Db};
use types::network_event::NetworkEvent;

#[async_trait::async_trait]
pub trait Handler<N: Network, D: Db, W: Wallet>: Send + Any {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError>;
}

impl<N: Network, D: Db, W: Wallet> dyn Handler<N, D, W> {
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        (self as &dyn Any).downcast_ref::<T>()
    }
}
