use futures::StreamExt;
use libp2p::mdns;

use libp2p::gossipsub;
use libp2p::request_response;
use libp2p::swarm::SwarmEvent;
use tokio::select;

use crate::NodeState;
use crate::errors::NodeError;
use crate::swarm_manager::MyBehaviourEvent;
use crate::swarm_manager::NetworkMessage;
use crate::swarm_manager::{PrivateRequest, PrivateResponse};

impl NodeState {
    pub async fn main_loop(&mut self) -> Result<(), NodeError> {
        // Read full lines from stdin
        let round1_topic = gossipsub::IdentTopic::new("round1_topic");
        self.swarm
            .inner
            .behaviour_mut()
            .gossipsub
            .subscribe(&round1_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        // let topic = gossipsub::IdentTopic::new("publish-key");
        // self.swarm.inner.behaviour_mut().gossipsub.subscribe(&topic)?;

        let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");
        self.swarm
            .inner
            .behaviour_mut()
            .gossipsub
            .subscribe(&start_dkg_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        println!("Local peer id: {}", self.peer_id);

        loop {
            select! {
                send_message = self.swarm.rx.recv() => match send_message {
                    Some(NetworkMessage::SendBroadcast { topic, message }) => {
                        let _ = self.swarm.inner
                            .behaviour_mut()
                            .gossipsub
                            .publish(topic, message);
                    }
                    Some(NetworkMessage::SendPrivateRequest(peer_id, request)) => {
                        self.swarm.inner
                            .behaviour_mut()
                            .request_response
                            .send_request(&peer_id, request);
                    }
                    Some(NetworkMessage::SendPrivateResponse(channel, response)) => {
                        let _ = self.swarm.inner
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, response);
                    }
                    Some(NetworkMessage::SendSelfRequest{ request }) => {
                        match request {
                            PrivateRequest::InsertBlock { hash, block } => {
                                match self.db.insert_block(hash, block) {
                                    Ok(_) => (),
                                    Err(e) => {
                                        return Err(NodeError::Error(format!("Failed to start signing session: {}", e)));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(NetworkMessage::SendSelfRequestSync{ request, response_channel }) => {
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
                    _ => {}
                },
                event = self.swarm.inner.select_next_some() => match event {
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        message,
                        ..
                    })) => {
                        match message.topic {
                            t if t == round1_topic.hash() => {
                                if let Some(source_peer) = message.source {
                                    println!("Received round1 payload from {}", self.peer_name(&source_peer));
                                } else {
                                    return Err(NodeError::Error("No source peer".to_string()));
                                }

                                if let Some(source_peer) = message.source {
                                    match self.dkg_state.handle_round1_payload(source_peer, message.data) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            println!("❌ Failed to handle round1 payload: {}", e);
                                        }
                                    }
                                }
                            }
                            t if t == start_dkg_topic.hash() => {
                                match self.dkg_state.handle_dkg_start() {
                                    Ok(_) => (),
                                    Err(e) => {
                                        println!("❌ Failed to handle DKG start: {}", e);
                                    }
                                }
                            }
                            _ => {
                                println!("Received unhandled broadcast");
                            }
                        }
                    },
                    // Handle direct message requests (incoming)
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::Round2Package(package), channel, .. }
                        }
                    )) => {
                        match self.dkg_state.handle_round2_payload(peer, package, channel) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("❌ Failed to handle round2 payload: {}", e);
                            }
                        }
                    },
                    // Incoming SignRequest
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::SignRequest { sign_id, message }, channel, .. }
                        }
                    )) => {
                        match self.handle_sign_request(peer, sign_id, message, channel) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("❌ Failed to handle sign request: {}", e);
                            }
                        }
                    },
                    // Incoming SignPackage request to generate signature share
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::SignPackage { sign_id, package }, channel, .. }
                        }
                    )) => {
                        match self.handle_sign_package(peer, sign_id, package, channel) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("❌ Failed to handle sign package: {}", e);
                            }
                        }
                    },
                    // Responses with commitments
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::Commitments { sign_id, commitments }, .. }
                        }
                    )) => {
                        match self.handle_commitments_response(peer, sign_id, commitments) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("❌ Failed to handle commitments response: {}", e);
                            }
                        }
                    },
                    // Responses with signature share
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::SignatureShare { sign_id, signature_share }, .. }
                        }
                    )) => {
                        match self.handle_signature_share(peer, sign_id, signature_share) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("❌ Failed to handle signature share: {}", e);
                            }
                        }
                    },
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        let peer_count = self.swarm.inner.behaviour().gossipsub.all_peers().count();
                        let peer_name = self.peer_name(&peer_id);
                        println!("Connection closed with peer: {peer_name}, peers: {peer_count}");
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed {
                        peer_id,
                        topic,
                    })) => {
                        if topic == start_dkg_topic.hash() {
                            self.dkg_state.dkg_listeners.insert(peer_id);
                            println!("Peer {} subscribed to topic {topic}. Listeners: {}", self.peer_name(&peer_id), self.dkg_state.dkg_listeners.len());
                            if let Err(e) = self.dkg_state.handle_dkg_start() {
                                println!("❌ Failed to handle DKG start: {}", e);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer_id, _multiaddr) in list {
                            if self.allowed_peers.contains(&peer_id) {
                                self.peers.insert(peer_id);
                                self.dkg_state.peers.insert(peer_id);
                                self.swarm.inner.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                        for (peer_id, _multiaddr) in list {
                            if self.allowed_peers.contains(&peer_id) {
                                self.peers.retain(|p| *p != peer_id);
                                self.dkg_state.peers.retain(|p| *p != peer_id);
                                self.swarm.inner.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::StartSigningSession{ hex_message }, channel, .. }
                        }
                    )) => {
                        if peer == self.peer_id {
                            match self.start_signing_session(&hex_message) {
                                Ok(_) => (),
                                Err(e) => {
                                    println!("❌ Failed to start signing session: {}", e);
                                }
                            }
                            let _ = self
                                .swarm.inner
                                .behaviour_mut()
                                .request_response
                                .send_response(channel, PrivateResponse::Pong);
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::Spend{ amount_sat }, channel, .. }
                        }
                    )) => {
                        println!("Spend request from peer: {}", self.peer_name(&peer));
                        if peer == self.peer_id {
                            self.start_spend_request(amount_sat);

                            let _ = self
                                .swarm.inner
                                .behaviour_mut()
                                .request_response
                                .send_response(channel, PrivateResponse::Pong);
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}
