use libp2p::gossipsub::{IdentTopic, Message};
use tracing::info;
use types::errors::NodeError;

use crate::{
    NodeState,
    db::Db,
    deposit_intents::{DepositIntent, DepositIntentState},
    handler::Handler,
    swarm_manager::{Network, NetworkEvent, SelfRequest, SelfResponse},
};

#[async_trait::async_trait]
impl<N: Network, D: Db> Handler<N, D> for DepositIntentState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D>,
        message: Option<NetworkEvent>,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::CreateDeposit { deposit_intent },
                response_channel,
            }) => {
                let response = self.create_deposit(node, deposit_intent).await;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::CreateDepositResponse {
                            success: response.is_ok(),
                        })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {}", e)))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::GetPendingDepositIntents,
                response_channel,
            }) => {
                let response = self.get_pending_deposit_intents();
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::GetPendingDepositIntentsResponse { intents: response })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {}", e)))?;
                }
            }
            Some(NetworkEvent::GossipsubMessage(Message { data, topic, .. })) => {
                if topic == IdentTopic::new("deposit-intents").hash() {
                    let deposit_intent =
                        serde_json::from_slice::<DepositIntent>(&data).map_err(|e| {
                            NodeError::Error(format!("Failed to parse deposit intent: {}", e))
                        })?;

                    if let Err(e) = self.create_deposit(node, deposit_intent).await {
                        info!("Failed to store deposit intent: {}", e);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
