use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::{Hash, Hasher},
    time::Duration,
};

use futures::stream::StreamExt;
use libp2p::{
    gossipsub, mdns, noise, request_response,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, PeerId, StreamProtocol,
};
use tokio::{io, io::AsyncBufReadExt, select};
use tracing_subscriber::EnvFilter;

use libp2p::request_response::cbor;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PingBody {
    pub message: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum PrivateRequest {
    Ping(PingBody),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PongBody {
    pub message: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum PrivateResponse {
    Pong(PongBody),
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
    request_response: cbor::Behaviour<PrivateRequest, PrivateResponse>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            // To content-address message, we can take the hash of message and use it as an ID.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
                .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
                .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                .mesh_n_low(1) // Minimum number of peers in mesh network (default is 4)
                .mesh_n_high(12) // Maximum number of peers in mesh network
                .mesh_n(3) // Target number of peers in mesh network (default is 6)
                .mesh_outbound_min(1) // Minimum outbound connections (default is 2)
                .gossip_lazy(3) // Number of peers to gossip to (default is 6)
                .flood_publish(true) // Always flood publish messages to all peers, regardless of mesh
                .build()
                .map_err(io::Error::other)?; // Temporary hack because `build` does not return a proper `std::error::Error`.

            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;

            let request_response = cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/direct-message/1.0.0"),
                    request_response::ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            );

            Ok(MyBehaviour {
                gossipsub,
                mdns,
                request_response,
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let local_peer_id = *swarm.local_peer_id();
    println!("Local peer ID: {}", local_peer_id);

    let topic = gossipsub::IdentTopic::new("publish-key");
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    println!("Enter messages via STDIN:");
    println!("- For broadcast: just type your message");
    println!("- For direct message: @<peer_id> <message>");
    println!("- To list connected peers: /peers");
    println!(
        "Network configured with lenient peer requirements to reduce 'insufficient peers' errors"
    );

    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                if line.trim() == "/peers" {
                    // List all connected peers
                    let connected_peers: Vec<_> = swarm.behaviour().gossipsub.all_peers().map(|(peer_id, _)| peer_id).collect();
                    println!("Connected peers ({}):", connected_peers.len());
                    for peer_id in connected_peers {
                        println!("  {}", peer_id);
                    }
                } else if line.starts_with('@') {
                    // Direct message to specific peer
                    let parts: Vec<&str> = line[1..].splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        let peer_id_str = parts[0];
                        let message_content = parts[1];

                        match peer_id_str.parse::<PeerId>() {
                            Ok(target_peer_id) => {
                                let direct_message = format!("From {}: {}", local_peer_id, message_content);

                                let request_id = swarm
                                    .behaviour_mut()
                                    .request_response
                                    .send_request(&target_peer_id, PrivateRequest::Ping(PingBody {
                                        message: direct_message.clone(),
                                    }));

                                println!("Sending direct message to {}: {}", target_peer_id, message_content);
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
                } else {
                    // Broadcast message via gossipsub
                    let peer_count = swarm.behaviour().gossipsub.all_peers().count();
                    println!("Connected peers: {}", peer_count);

                    if let Err(e) = swarm
                        .behaviour_mut().gossipsub
                        .publish(topic.clone(), line.as_bytes()) {
                        println!("Publish error: {e:?}");
                    } else {
                        println!("Message published successfully to {} peers", peer_count);
                    }
                }
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discovered a new peer: {peer_id}");
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discover peer has expired: {peer_id}");
                        swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message,
                    ..
                })) => println!(
                        "📢 Broadcast from {}: '{}'",
                        peer_id,
                        String::from_utf8_lossy(&message.data),
                    ),
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
                // Handle direct message requests (incoming)
                SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                    request_response::Event::Message {
                        peer,
                        message: request_response::Message::Request { request: PrivateRequest::Ping(PingBody { message }), channel, .. }
                    }
                )) => {
                    println!("💬 Direct message from {}: '{}'", peer, message);

                    // Send acknowledgment
                    let response = swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, PrivateResponse::Pong(PongBody { message: format!("Message received by {}", local_peer_id)}));
                },
                // Handle direct message responses (outgoing message acknowledgments)
                SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(
                    request_response::Event::Message {
                        peer,
                        message: request_response::Message::Response { response: PrivateResponse::Pong(PongBody{ message }), .. }
                    }
                )) => {
                    println!("✅ Message delivered to {}: {}", peer, message);
                },
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!("Connection established with peer: {peer_id}");
                    let peer_count = swarm.behaviour().gossipsub.all_peers().count();
                    println!("Total connected peers: {}", peer_count);
                },
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    println!("Connection closed with peer: {peer_id}");
                    let peer_count = swarm.behaviour().gossipsub.all_peers().count();
                    println!("Total connected peers: {}", peer_count);
                },
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Local node is listening on {address}");
                }
                _ => {}
            }
        }
    }
}
