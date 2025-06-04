use futures::StreamExt;
use libp2p::PeerId;
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
use crate::swarm_manager::{PingBody, PrivateRequest, PrivateResponse};

impl NodeState {
    pub fn handle_input(&mut self, line: String) {
        if line.trim() == "/dkg" {
            // Create start-dkg topic
            let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");

            // Send a message to start DKG
            let start_message = format!("START_DKG:{}", self.peer_id);
            let _ = self
                .swarm
                .behaviour_mut()
                .gossipsub
                .publish(start_dkg_topic.clone(), start_message.as_bytes());

            self.handle_dkg_start();

            println!("Sent DKG start signal");
        } else if line.trim() == "/peers" {
            let connected_peers: Vec<_> = self
                .swarm
                .behaviour()
                .gossipsub
                .all_peers()
                .map(|(peer_id, _)| peer_id)
                .collect();
            println!("Connected peers ({}):", connected_peers.len());
            for peer_id in connected_peers {
                println!("  {}", peer_id);
            }
        } else if let Some(amount_str) = line.trim().strip_prefix("/spend ") {
            match amount_str.trim().parse::<u64>() {
                Ok(amount_sat) => {
                    self.handle_spend_request(amount_sat);
                }
                Err(e) => println!("❌ Invalid amount: {}", e),
            }
        } else if let Some(hex_msg) = line.strip_prefix("/sign ") {
            self.start_signing_session(hex_msg.trim());
        } else if let Some(stripped) = line.strip_prefix('@') {
            let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let peer_id_str = parts[0];
                let message_content = parts[1];

                match peer_id_str.parse::<PeerId>() {
                    Ok(target_peer_id) => {
                        let direct_message = format!("From {}: {}", self.peer_id, message_content);

                        let request_id = self.swarm.behaviour_mut().request_response.send_request(
                            &target_peer_id,
                            PrivateRequest::Ping(PingBody {
                                message: direct_message.clone(),
                            }),
                        );

                        println!(
                            "Sending direct message to {}: {}",
                            target_peer_id, message_content
                        );
                        println!("Request ID: {:?}", request_id);
                    }
                    Err(e) => {
                        println!("Invalid peer ID format: {}", e);
                        println!("Usage: @<peer_id> <message>");
                    }
                }
            } else {
                println!("Usage: @<peer_id> <message>");
            }
        }
    }

    pub async fn main_loop(&mut self) -> Result<(), NodeError> {
        // Read full lines from stdin
        let mut stdin = io::BufReader::new(io::stdin()).lines();

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
                Ok(Some(line)) = stdin.next_line() => {
                    self.handle_input(line);
                }
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
