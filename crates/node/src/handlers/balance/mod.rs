use crate::{Network, NodeState, handlers::Handler, wallet::Wallet};
use types::errors::NodeError;
use types::network_event::{NetworkEvent, SelfRequest, SelfResponse};

#[derive(Default)]
pub struct BalanceState;

impl BalanceState {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for BalanceState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        if let Some(NetworkEvent::SelfRequest {
            request: SelfRequest::CheckBalance { address },
            response_channel,
        }) = message
        {
            let balance = node
                .chain_interface
                .get_account(&address)
                .map_or(0, |acct| acct.balance);

            if let Some(response_channel) = response_channel {
                response_channel
                    .send(SelfResponse::CheckBalanceResponse {
                        balance_satoshis: balance,
                    })
                    .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
            }
        }
        Ok(())
    }
}
