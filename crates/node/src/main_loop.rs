use libp2p::request_response;
use tracing::{error, info};

use crate::db::Db;
use crate::swarm_manager::{DirectMessage, NetworkEvent, SelfRequest, SelfResponse};
use crate::{Network, NodeState};
use types::errors::NodeError;

impl<N: Network, D: Db> NodeState<N, D> {
    pub async fn try_poll(&mut self) -> Result<bool, NodeError> {
        let send_message = self.network_events_stream.try_recv().ok();
        if let Some(event) = send_message {
            self.handle(Some(event)).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn poll(&mut self) -> Result<(), NodeError> {
        let send_message = self.network_events_stream.recv().await;
        self.handle(send_message).await
    }

    pub async fn start(&mut self) -> Result<(), NodeError> {
        info!("Local peer id: {}", self.peer_id);

        loop {
            self.poll().await?
        }
    }

    pub async fn handle(&mut self, send_message: Option<NetworkEvent>) -> Result<(), NodeError> {
        for handler in self.handlers.iter_mut() {
            let handler_message = send_message.as_ref().map(|event| event.into());
            handler
                .handle(handler_message, &self.network_handle)
                .await?;
        }

        match send_message {
            Some(NetworkEvent::SelfRequest {
                request,
                response_channel,
            }) => match request {
                SelfRequest::StartSigningSession { hex_message } => {
                    self.start_signing_session(&hex_message)?;
                }
                SelfRequest::InsertBlock { block } => {
                    self.db.insert_block(block)?;
                }
                SelfRequest::Spend { amount_sat } => {
                    let response = self.start_spend_request(amount_sat);
                    if let Some(response_channel) = response_channel {
                        response_channel
                            .send(SelfResponse::SpendRequestSent {
                                sighash: response.unwrap_or("No sighash".to_string()),
                            })
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send response: {}", e))
                            })?;
                    }
                }
                SelfRequest::GetFrostPublicKey => {
                    let response = self.get_frost_public_key();
                    if let Some(response_channel) = response_channel {
                        response_channel
                            .send(SelfResponse::GetFrostPublicKeyResponse {
                                public_key: response,
                            })
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send response: {}", e))
                            })?;
                    }
                }
                SelfRequest::SetFrostKeys {
                    private_key,
                    public_key,
                } => {
                    self.set_frost_keys(private_key, public_key)?;
                }
            },
            Some(NetworkEvent::PeersConnected(list)) => {
                for (peer_id, _multiaddr) in list {
                    self.peers.insert(peer_id);
                }
            }
            // Handle direct message requests (incoming)
            Some(NetworkEvent::MessageEvent(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request: DirectMessage::SignRequest { sign_id, message },
                        ..
                    },
            })) => match self.handle_sign_request(peer, sign_id, message) {
                Ok(_) => (),
                Err(e) => {
                    error!("❌ Failed to handle sign request: {}", e);
                }
            },
            // Handle direct message requests (incoming)
            Some(NetworkEvent::MessageEvent(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request: DirectMessage::SignPackage { sign_id, package },
                        ..
                    },
            })) => match self.handle_sign_package(peer, sign_id, package) {
                Ok(_) => (),
                Err(e) => {
                    error!("❌ Failed to handle sign package: {}", e);
                }
            },
            // Handle direct message requests (incoming)
            Some(NetworkEvent::MessageEvent(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request:
                            DirectMessage::Commitments {
                                sign_id,
                                commitments,
                            },
                        ..
                    },
            })) => match self.handle_commitments_response(peer, sign_id, commitments) {
                Ok(_) => (),
                Err(e) => {
                    error!("❌ Failed to handle commitments response: {}", e);
                }
            },
            // Handle direct message requests (incoming)
            Some(NetworkEvent::MessageEvent(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request:
                            DirectMessage::SignatureShare {
                                sign_id,
                                signature_share,
                            },
                        ..
                    },
            })) => match self.handle_signature_share(peer, sign_id, signature_share) {
                Ok(_) => (),
                Err(e) => {
                    error!("❌ Failed to handle signature share: {}", e);
                }
            },
            _ => {}
        }
        Ok(())
    }
}
