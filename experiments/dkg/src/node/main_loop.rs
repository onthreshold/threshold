use futures::StreamExt;
use libp2p::mdns;
use tokio::io;

use frost_secp256k1::{self as frost};
use libp2p::gossipsub;
use libp2p::request_response;
use libp2p::swarm::SwarmEvent;
use tokio::io::AsyncBufReadExt;
use tokio::select;

use crate::node::NodeState;
use crate::swarm_manager::MyBehaviourEvent;
use crate::swarm_manager::{PingBody, PrivateRequest, PrivateResponse};

impl<'a> NodeState<'a> {
    pub async fn main_loop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Read full lines from stdin
        let mut stdin = io::BufReader::new(io::stdin()).lines();

        let round1_topic = gossipsub::IdentTopic::new("round1_topic");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&round1_topic)?;

        // let topic = gossipsub::IdentTopic::new("publish-key");
        // self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

        let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&start_dkg_topic)?;

        loop {
            select! {
                Ok(Some(line)) = stdin.next_line() => {
                    self.handle_input(line, &round1_topic);
                }
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        message,
                        ..
                    })) => {
                        match message.topic {
                            t if t == round1_topic.hash() => {
                                let data = frost::keys::dkg::round1::Package::deserialize(&message.data)
                                    .expect("Failed to deserialize round1 package");
                                if let Some(source_peer) = message.source {
                                    self.handle_round1_payload(source_peer, data);
                                }
                            }
                            t if t == start_dkg_topic.hash() => {
                                self.handle_dkg_start(&round1_topic);
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
                            message: request_response::Message::Request { request: PrivateRequest::Ping(PingBody { message }), channel, .. }
                        }
                    )) => {
                        println!("💬 Direct message from {}: '{}'", peer, message);

                        // Send acknowledgment
                        let _response = self
                            .swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, PrivateResponse::Pong);
                    },
                    // Handle direct message responses (outgoing message acknowledgments)
                    SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            peer,
                            message: request_response::Message::Response { response: PrivateResponse::Pong, .. }
                        }
                    )) => {
                        println!("✅ Message delivered to {}", peer);
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
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        println!("Connection established with peer: {peer_id}");
                        let peer_count = self.swarm.behaviour().gossipsub.all_peers().count();
                        println!("Total connected peers: {}", peer_count);
                    },
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        println!("Connection closed with peer: {peer_id}");
                        let peer_count = self.swarm.behaviour().gossipsub.all_peers().count();
                        println!("Total connected peers: {}", peer_count);
                    },
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Local node is listening on {address}");
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed {
                        peer_id,
                        topic,
                    })) => {
                        println!("Peer {peer_id} subscribed to topic {topic}");
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Unsubscribed {
                        peer_id,
                        topic,
                    })) => {
                        println!("Peer {peer_id} unsubscribed from topic {topic}");
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer_id, _multiaddr) in list {
                            println!("mDNS discovered a new peer: {peer_id}");
                            self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                        for (peer_id, _multiaddr) in list {
                            println!("mDNS discover peer has expired: {peer_id}");
                            self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                        }
                    },
                    _ => {
                        println!("Swarm event: {event:?}");
                    }
                }
            }
        }
    }
}
