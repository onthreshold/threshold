use crate::wallet::Wallet;
use crate::{NodeState, handlers::Handler, handlers::signing::SigningState};
use types::errors::NodeError;
use types::network::network_event::{DirectMessage, NetworkEvent, SelfRequest, SelfResponse};
use types::network::network_protocol::Network;

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for SigningState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: NetworkEvent,
    ) -> Result<(), NodeError> {
        match message {
            NetworkEvent::SelfRequest {
                request: SelfRequest::StartSigningSession { hex_message },
                ..
            } => {
                let _ = self.start_signing_session(node, &hex_message)?;
            }
            NetworkEvent::SelfRequest {
                request:
                    SelfRequest::Spend {
                        amount_sat,
                        fee,
                        address_to,
                        user_pubkey,
                    },
                response_channel,
            } => {
                let response = self.start_spend_request(
                    node,
                    amount_sat,
                    fee,
                    &address_to,
                    user_pubkey,
                    false,
                );
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::SpendRequestSent {
                            sighash: response.unwrap_or_else(|| "No sighash".to_string()),
                        })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
                }
            }
            NetworkEvent::MessageEvent((peer, DirectMessage::SignRequest { sign_id, message })) => {
                self.handle_sign_request(node, peer, sign_id, message)?;
            }
            NetworkEvent::MessageEvent((peer, DirectMessage::SignPackage { sign_id, package })) => {
                self.handle_sign_package(node, peer, sign_id, &package)?;
            }
            NetworkEvent::MessageEvent((
                peer,
                DirectMessage::Commitments {
                    sign_id,
                    commitments,
                },
            )) => self.handle_commitments_response(node, peer, sign_id, &commitments)?,
            NetworkEvent::MessageEvent((
                peer,
                DirectMessage::SignatureShare {
                    sign_id,
                    signature_share,
                },
            )) => {
                self.handle_signature_share(node, peer, sign_id, &signature_share)
                    .await?;
            }
            _ => (),
        }

        Ok(())
    }
}
