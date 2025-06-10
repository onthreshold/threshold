use crate::swarm_manager::{Network, NetworkEvent, SelfRequest, SelfResponse};
use crate::{NodeState, db::Db, handler::Handler, withdrawl::SpendIntentState};
use protocol::oracle::Oracle;
use types::errors::NodeError;

#[async_trait::async_trait]
impl<N: Network, D: Db, O: Oracle> Handler<N, D, O> for SpendIntentState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, O>,
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
                self.confirm_withdrawal(node, &challenge, &signature)
                    .await?;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::ConfirmWithdrawalResponse { success: true })
                        .map_err(|e| NodeError::Error(e.to_string()))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
