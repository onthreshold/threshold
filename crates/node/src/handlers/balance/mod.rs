use crate::{Network, NodeState, db::Db, handlers::Handler, wallet::Wallet};
use types::errors::NodeError;
use types::network_event::{NetworkEvent, SelfRequest, SelfResponse};

#[derive(Default)]
pub struct BalanceState;

impl BalanceState {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl<N: Network, D: Db, W: Wallet> Handler<N, D, W> for BalanceState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, W>,
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
