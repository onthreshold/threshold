use crate::{Network, NodeState, handlers::Handler, wallet::Wallet};
use abci::chain_state::Account;
use abci::{ChainMessage, ChainResponse};
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
            let ChainResponse::GetAccount { account } = node
                .chain_interface_tx
                .send_message_with_response(ChainMessage::GetAccount {
                    address: address.clone(),
                })
                .await?
            else {
                return Err(NodeError::Error("Failed to get account".to_string()));
            };

            let account = account.unwrap_or_else(|| Account::new(address, 0));

            let balance = account.balance;

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
