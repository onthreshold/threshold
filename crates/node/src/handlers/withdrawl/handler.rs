use crate::wallet::Wallet;
use crate::{NodeState, handlers::Handler, handlers::withdrawl::SpendIntentState};
use libp2p::gossipsub::Message;
use types::errors::NodeError;
use types::intents::PendingSpend;
use types::network::network_event::{NetworkEvent, SelfRequest, SelfResponse};
use types::network::network_protocol::Network;
use types::proto::ProtoDecode;

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for SpendIntentState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        match message {
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::ProposeWithdrawal { withdrawal_intent },
                response_channel,
            }) => {
                let (total_amount, challenge) =
                    self.propose_withdrawal(node, &withdrawal_intent).await?;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::ProposeWithdrawalResponse {
                            quote_satoshis: total_amount,
                            challenge,
                        })
                        .map_err(|e| NodeError::Error(e.to_string()))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request:
                    SelfRequest::ConfirmWithdrawal {
                        challenge,
                        signature,
                    },
                response_channel,
            }) => {
                self.confirm_withdrawal(node, &challenge, &signature)?;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::ConfirmWithdrawalResponse { success: true })
                        .map_err(|e| NodeError::Error(e.to_string()))?;
                }
            }
            Some(NetworkEvent::GossipsubMessage(Message { data, topic, .. })) => {
                if topic.as_str() == "withdrawls" {
                    let spend_intent: PendingSpend =
                        PendingSpend::decode(&data).map_err(NodeError::Error)?;
                    self.handle_withdrawl_message(node, spend_intent).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
