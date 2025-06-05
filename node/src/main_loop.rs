use libp2p::mdns;

use libp2p::gossipsub;
use libp2p::request_response;
use libp2p::swarm::SwarmEvent;
use tokio::select;
use tracing::{debug, error, info};

use crate::NodeState;
use crate::errors::NodeError;
use crate::swarm_manager::{
    MyBehaviourEvent, NetworkEvent, NetworkMessage, PrivateRequest, PrivateResponse,
};

impl NodeState {
    pub async fn start(&mut self) -> Result<(), NodeError> {
        info!("Local peer id: {}", self.peer_id);

        let round1_topic = gossipsub::IdentTopic::new("round1_topic");
        let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");
        loop {
            select! {
                send_message = self.network_events_stream.recv() => match send_message {
                    Some(NetworkEvent::NetworkMessage(NetworkMessage::SendSelfRequest { request, response_channel: None })) => {
                        debug!("Received self request {:?}", request);
                            match request {
                                PrivateRequest::InsertBlock { block } => {
                                    match self.db.insert_block(block) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            return Err(NodeError::Error(format!("Failed to handle genesis block: {}", e)));
                                        }
                                    }
                                }
                                _ => {}
                            }
                    }
                    Some(NetworkEvent::NetworkMessage(NetworkMessage::SendSelfRequest { request, response_channel: Some(response_channel) })) => {
                        debug!("Received self request {:?}", request);
                            match request {
                                PrivateRequest::StartSigningSession { hex_message } => {
                                    match self.start_signing_session(&hex_message) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            return Err(NodeError::Error(format!("Failed to start signing session: {}", e)));
                                        }
                                    }
                                },
                                PrivateRequest::Spend { amount_sat } => {
                                    let response = self.start_spend_request(amount_sat);
                                    match response_channel.send(PrivateResponse::SpendRequestSent { sighash: response.unwrap_or("No sighash".to_string()) }) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            return Err(NodeError::Error(format!("Failed to send response: {}", e)));
                                        }
                                    }
                                }
                                PrivateRequest::GetFrostPublicKey => {
                                    let response = self.get_frost_public_key();
                                    match response_channel.send(PrivateResponse::GetFrostPublicKey { public_key: response.unwrap_or("No public key".to_string()) }) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            return Err(NodeError::Error(format!("Failed to send response: {}", e)));
                                        }
                                    }
                                }
                                _ => {}
                            }
                    }
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))))) => {
                        for (peer_id, _multiaddr) in list {
                            self.peers.insert(peer_id);
                            self.dkg_state.peers.insert(peer_id);
                        }
                    },
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        message,
                        ..
                    })))) => {
                        match message.topic {
                            t if t == round1_topic.hash() => {
                                if let Some(source_peer) = message.source {
                                    info!("Received round1 payload from {}", self.peer_name(&source_peer));
                                } else {
                                    return Err(NodeError::Error("No source peer".to_string()));
                                }

                                if let Some(source_peer) = message.source {
                                    match self.dkg_state.handle_round1_payload(source_peer, message.data) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            error!("❌ Failed to handle round1 payload: {}", e);
                                        }
                                    }
                                }
                            }
                            t if t == start_dkg_topic.hash() => {
                                match self.dkg_state.handle_dkg_start() {
                                    Ok(_) => (),
                                    Err(e) => {
                                        error!("❌ Failed to handle DKG start: {}", e);
                                    }
                                }
                            }
                            _ => {
                                debug!("Received unhandled broadcast");
                            }
                        }
                    },
                    // Handle direct message requests (incoming)
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::Round2Package(package), channel, .. }
                        }
                    )))) => {
                        match self.dkg_state.handle_round2_payload(peer, package, channel) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("❌ Failed to handle round2 payload: {}", e);
                            }
                        }
                    },
                    // Incoming SignRequest
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::SignRequest { sign_id, message }, channel, .. }
                        }
                    )))) => {
                        match self.handle_sign_request(peer, sign_id, message, channel) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("❌ Failed to handle sign request: {}", e);
                            }
                        }
                    },
                    // Incoming SignPackage request to generate signature share
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::SignPackage { sign_id, package }, channel, .. }
                        }
                    )))) => {
                        match self.handle_sign_package(peer, sign_id, package, channel) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("❌ Failed to handle sign package: {}", e);
                            }
                        }
                    },
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed {
                        peer_id,
                        topic,
                    })))) => {
                        if topic == start_dkg_topic.hash() {
                            self.dkg_state.dkg_listeners.insert(peer_id);
                            info!("Peer {} subscribed to topic {topic}. Listeners: {}", self.peer_name(&peer_id), self.dkg_state.dkg_listeners.len());
                            if let Err(e) = self.dkg_state.handle_dkg_start() {
                                error!("❌ Failed to handle DKG start: {}", e);
                            }
                        }
                    },
                    // Responses with commitments
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::Commitments { sign_id, commitments }, .. }
                        }
                    )))) => {
                        match self.handle_commitments_response(peer, sign_id, commitments) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("❌ Failed to handle commitments response: {}", e);
                            }
                        }
                    },
                    // Responses with signature share
                    Some(NetworkEvent::SwarmEvent(SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::SignatureShare { sign_id, signature_share }, .. }
                        }
                    )))) => {
                        match self.handle_signature_share(peer, sign_id, signature_share) {
                            Ok(_) => (),
                            Err(e) => {
                                error!("❌ Failed to handle signature share: {}", e);
                            }
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}
