use crate::NodeState;
use crate::db::Db;
use crate::handlers::Handler;
use crate::handlers::signing::SigningState;
use crate::swarm_manager::{DirectMessage, Network, NetworkEvent, SelfRequest, SelfResponse};
use protocol::oracle::Oracle;
use types::errors::NodeError;

#[async_trait::async_trait]
impl<N: Network, D: Db, O: Oracle> Handler<N, D, O> for SigningState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, O>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        match message {
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::StartSigningSession { hex_message },
                ..
            }) => {
                let _ = self.start_signing_session(node, &hex_message)?;
            }
            Some(NetworkEvent::SelfRequest {
                request:
                    SelfRequest::Spend {
                        amount_sat,
                        address_to,
                    },
                response_channel,
            }) => {
                let response = self.start_spend_request(node, amount_sat, &address_to);
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::SpendRequestSent {
                            sighash: response.unwrap_or("No sighash".to_string()),
                        })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {}", e)))?;
                }
            }
            Some(NetworkEvent::MessageEvent((
                peer,
                DirectMessage::SignRequest { sign_id, message },
            ))) => self.handle_sign_request(node, peer, sign_id, message)?,
            Some(NetworkEvent::MessageEvent((
                peer,
                DirectMessage::SignPackage { sign_id, package },
            ))) => self.handle_sign_package(node, peer, sign_id, package)?,
            Some(NetworkEvent::MessageEvent((
                peer,
                DirectMessage::Commitments {
                    sign_id,
                    commitments,
                },
            ))) => self.handle_commitments_response(node, peer, sign_id, commitments)?,
            Some(NetworkEvent::MessageEvent((
                peer,
                DirectMessage::SignatureShare {
                    sign_id,
                    signature_share,
                },
            ))) => self.handle_signature_share(node, peer, sign_id, signature_share)?,
            _ => (),
        }

        Ok(())
    }
}
