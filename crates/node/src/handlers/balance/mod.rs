use crate::{
    Network, NodeState,
    db::Db,
    handlers::Handler,
    swarm_manager::{NetworkEvent, SelfRequest, SelfResponse},
    wallet::Wallet,
};
use protocol::oracle::Oracle;
use types::errors::NodeError;

#[derive(Default)]
pub struct BalanceState;

impl BalanceState {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl<N: Network, D: Db, O: Oracle, W: Wallet<O>> Handler<N, D, O, W> for BalanceState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, O, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        if let Some(NetworkEvent::SelfRequest {
            request: SelfRequest::CheckBalance { address },
            response_channel,
        }) = message
        {
            let balance = node
                .chain_state
                .get_account(&address)
                .map(|acct| acct.balance)
                .unwrap_or(0);

            if let Some(response_channel) = response_channel {
                response_channel
                    .send(SelfResponse::CheckBalanceResponse {
                        balance_satoshis: balance,
                    })
                    .map_err(|e| NodeError::Error(format!("Failed to send response: {}", e)))?;
            }
        }
        Ok(())
    }
}
