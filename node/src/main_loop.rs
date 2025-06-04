use futures::StreamExt;
use libp2p::mdns;
use tokio::io;

use frost_secp256k1::{self as frost};
use libp2p::gossipsub;
use libp2p::request_response;
use libp2p::swarm::SwarmEvent;
use tokio::io::AsyncBufReadExt;
use tokio::select;

use crate::NodeState;
use crate::errors::NodeError;
use crate::swarm_manager::MyBehaviourEvent;
use crate::swarm_manager::{PrivateRequest, PrivateResponse};

impl NodeState {
    pub async fn main_loop(&mut self) -> Result<(), NodeError> {
        // Read full lines from stdin
        let round1_topic = gossipsub::IdentTopic::new("round1_topic");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&round1_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        // let topic = gossipsub::IdentTopic::new("publish-key");
        // self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

        let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&start_dkg_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        println!("Local peer id: {}", self.peer_id);

        loop {
            select! {
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        message,
                        ..
                    })) => {
                        match message.topic {
                            t if t == round1_topic.hash() => {
                                println!("Received round1 payload from {}", self.peer_name(&message.source.unwrap()));
                                let data = frost::keys::dkg::round1::Package::deserialize(&message.data)
                                    .expect("Failed to deserialize round1 package");
                                if let Some(source_peer) = message.source {
                                    self.handle_round1_payload(source_peer, data);
                                }
                            }
                            t if t == start_dkg_topic.hash() => {
                                self.handle_dkg_start();
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
                        self.handle_round2_payload(peer, package, channel);
                    },
                    // Incoming SignRequest
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::SignRequest { sign_id, message }, channel, .. }
                        }
                    )) => {
                        self.handle_sign_request(peer, sign_id, message, channel);
                    },
                    // Incoming SignPackage request to generate signature share
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Request { request: PrivateRequest::SignPackage { sign_id, package }, channel, .. }
                        }
                    )) => {
                        self.handle_sign_package(peer, sign_id, package, channel);
                    },
                    // Responses with commitments
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::Commitments { sign_id, commitments }, .. }
                        }
                    )) => {
                        self.handle_commitments_response(peer, sign_id, commitments);
                    },
                    // Responses with signature share
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::SignatureShare { sign_id, signature_share }, .. }
                        }
                    )) => {
                        self.handle_signature_share(peer, sign_id, signature_share);
                    },
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        let peer_count = self.swarm.behaviour().gossipsub.all_peers().count();
                        let peer_name = self.peer_name(&peer_id);
                        println!("Connection closed with peer: {peer_name}, peers: {peer_count}");
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed {
                        peer_id,
                        topic,
                    })) => {
                        if topic == start_dkg_topic.hash() {
                            self.dkg_listeners.insert(peer_id);
                            println!("Peer {} subscribed to topic {topic}. Listeners: {}", self.peer_name(&peer_id), self.dkg_listeners.len());
                            self.handle_dkg_start();
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer_id, _multiaddr) in list {
                            if self.allowed_peers.contains(&peer_id) {
                                self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                        for (peer_id, _multiaddr) in list {
                            if self.allowed_peers.contains(&peer_id) {
                                self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                            }
                        }
                    },
                    _ => {
                        // println!("Swarm event: {event:?}");
                    }
                }
            }
        }
    }
}
