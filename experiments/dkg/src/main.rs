use crate::swarm_manager::{MyBehaviourEvent, PingBody, PrivateRequest, PrivateResponse};
use frost_secp256k1::{self as frost};
use libp2p::{futures::StreamExt, gossipsub, mdns, request_response, swarm::SwarmEvent};
use swarm_manager::build_swarm;
use tokio::{
    io::{self, AsyncBufReadExt},
    select,
};

mod node;
mod swarm_manager;

use node::{NodeState, handle_input};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let max_signers = 3;
    let min_signers = 2;
    let mut swarm = build_swarm()
        .map_err(|e| println!("Failed to build swarm {}", e.message))
        .expect("Failed to build swarm");

    let round1_topic = gossipsub::IdentTopic::new("round1_topic");
    swarm.behaviour_mut().gossipsub.subscribe(&round1_topic)?;

    let topic = gossipsub::IdentTopic::new("publish-key");
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&start_dkg_topic)?;

    // Node State
    let local_peer_id = *swarm.local_peer_id();

    let mut node_state = NodeState::new(local_peer_id, &mut swarm, min_signers, max_signers);

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                handle_input(line, &mut node_state, &round1_topic);
            }
            event = node_state.swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message { propagation_source: peer_id,
                    message,
                    ..
                })) => {
                    match message.topic {
                        t if t == round1_topic.hash() => {
                            let data = frost::keys::dkg::round1::Package::deserialize(&message.data)
                                .expect("Failed to deserialize round1 package");
                            if let Some(source_peer) = message.source {
                                node_state.handle_part1_payload(source_peer, data);
                            }
                        }
                        t if t == start_dkg_topic.hash() => {
                            node_state.handle_dkg_start(&round1_topic);
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
                    let _response = node_state
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
                    node_state.handle_part2_payload(peer, package, channel);
                },
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!("Connection established with peer: {peer_id}");
                    let peer_count = node_state.swarm.behaviour().gossipsub.all_peers().count();
                    println!("Total connected peers: {}", peer_count);
                },
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    println!("Connection closed with peer: {peer_id}");
                    let peer_count = node_state.swarm.behaviour().gossipsub.all_peers().count();
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
                        node_state.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discover peer has expired: {peer_id}");
                        node_state.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                    }
                },
                _ => {
                    println!("Swarm event: {event:?}");
                }
            }
        }
    }
}
