use libp2p::gossipsub::{IdentTopic, Message};
use tracing::info;
use types::errors::NodeError;
use types::intents::DepositIntent;
use types::network_event::{NetworkEvent, SelfRequest, SelfResponse};

use crate::swarm_manager::Network;
use crate::{
    NodeState, db::Db, handlers::Handler, handlers::deposit::DepositIntentState, wallet::Wallet,
};

#[async_trait::async_trait]
impl<N: Network, D: Db, W: Wallet> Handler<N, D, W> for DepositIntentState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            Some(NetworkEvent::SelfRequest {
                request:
                    SelfRequest::CreateDeposit {
                        user_pubkey,
                        amount_sat,
                    },
                response_channel,
            }) => {
                let (deposit_tracking_id, deposit_address) =
                    self.create_deposit(node, user_pubkey, amount_sat).await?;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::CreateDepositResponse {
                            deposit_tracking_id,
                            deposit_address,
                        })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::GetPendingDepositIntents,
                response_channel,
            }) => {
                let response = self.get_pending_deposit_intents(node);
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::GetPendingDepositIntentsResponse { intents: response })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::ConfirmDeposit { confirmed_tx },
                ..
            }) => {
                if let Err(e) = self.update_user_balance(node, confirmed_tx) {
                    info!("Failed to update user balance: {}", e);
                }
            }
            Some(NetworkEvent::GossipsubMessage(Message { data, topic, .. })) => {
                if topic == IdentTopic::new("deposit-intents").hash() {

                    let deposit_intent = DepositIntent::decode(&data).map_err(|e| {
                        NodeError::Error(format!("Failed to parse deposit intent: {}", e))
                    })?;

                    if let Err(e) = self.create_deposit_from_intent(node, deposit_intent) {
                        info!("Failed to store deposit intent: {}", e);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
