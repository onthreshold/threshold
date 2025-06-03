use crate::swarm_manager::{MyBehaviourEvent, PingBody, PrivateRequest, PrivateResponse};
use frost_secp256k1::Identifier;
use frost_secp256k1::keys::dkg::round2;
use frost_secp256k1::{self as frost, keys::dkg::round1};
use libp2p::gossipsub::IdentTopic;
use libp2p::{PeerId, gossipsub};
use libp2p::{futures::StreamExt, mdns, request_response, swarm::SwarmEvent};
use std::collections::BTreeMap;
use tokio::io::{self, AsyncBufReadExt};
use tokio::select;

pub struct NodeState<'a> {
    pub r1_secret_package: Option<round1::SecretPackage>,
    pub peer_id: PeerId,
    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,
    pub peers: Vec<PeerId>,
    pub swarm: &'a mut libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub r2_secret_package: Option<round2::SecretPackage>,

    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,
}

impl<'a> NodeState<'a> {
    pub fn new(
        swarm: &'a mut libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
        min_signers: u16,
        max_signers: u16,
    ) -> Self {
        // Node State
        let peer_id = *swarm.local_peer_id();

        NodeState {
            r1_secret_package: None,
            r2_secret_package: None,
            peer_id,
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            swarm,
            min_signers,
            max_signers,
            peers: Vec::new(),
            rng: frost::rand_core::OsRng,
            pubkey_package: None,
            private_key_package: None,
        }
    }

    pub fn handle_dkg_start(&mut self, round1_topic: &IdentTopic) {
        // Run the DKG initialization code
        let participant_identifier = peer_id_to_identifier(&self.peer_id);

        let (round1_secret_package, round1_package) = frost::keys::dkg::part1(
            participant_identifier,
            self.max_signers,
            self.min_signers,
            self.rng,
        )
        .expect("Failed to generate round1 package");

        self.r1_secret_package = Some(round1_secret_package);

        let round1_package_bytes = round1_package
            .serialize()
            .expect("Failed to serialize round1 package");

        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(round1_topic.clone(), round1_package_bytes);

        self.try_enter_round2();

        println!(
            "Generated and published round1 package in response to DKG start signal {}",
            self.peer_id
        );
    }

    pub fn handle_round1_payload(&mut self, sender_peer_id: PeerId, package: round1::Package) {
        self.peers.push(sender_peer_id);
        // add package to peer packages
        self.round1_peer_packages
            .insert(peer_id_to_identifier(&sender_peer_id), package);

        println!(
            "Received round1 package from {} {}",
            sender_peer_id,
            self.round1_peer_packages.len()
        );

        self.try_enter_round2();
    }

    pub fn try_enter_round2(&mut self) {
        if let Some(r1_secret_package) = self.r1_secret_package.as_ref() {
            if self.round1_peer_packages.len() + 1 == self.max_signers as usize {
                println!("Received all round1 packages, entering part2");
                // all packages received
                let part2_result =
                    frost::keys::dkg::part2(r1_secret_package.clone(), &self.round1_peer_packages);
                match part2_result {
                    Ok((round2_secret_package, round2_packages)) => {
                        println!("Successfully completed step 1");
                        self.r1_secret_package = None;
                        self.r2_secret_package = Some(round2_secret_package);
                        for peer_to_send_to in self.peers.iter() {
                            let identifier = peer_id_to_identifier(peer_to_send_to);
                            let package_to_send = round2_packages.get(&identifier).unwrap();

                            let request = PrivateRequest::Round2Package(package_to_send.clone());

                            let _ = self
                                .swarm
                                .behaviour_mut()
                                .request_response
                                .send_request(peer_to_send_to, request);

                            println!("Sent round2 package to {}", peer_to_send_to);
                        }
                    }
                    Err(e) => {
                        println!("DKG failed: {}", e);
                    }
                }
            }
        }
    }

    pub fn handle_round2_payload(
        &mut self,
        sender_peer_id: PeerId,
        package: round2::Package,
        response_channel: libp2p::request_response::ResponseChannel<PrivateResponse>,
    ) {
        println!(
            "Received round2 package from {} {}",
            sender_peer_id,
            self.round1_peer_packages.len()
        );

        // add package to peer packages
        self.round2_peer_packages
            .insert(peer_id_to_identifier(&sender_peer_id), package);

        let _ = self
            .swarm
            .behaviour_mut()
            .request_response
            .send_response(response_channel, PrivateResponse::Pong);

        if let Some(r2_secret_package) = self.r2_secret_package.as_ref() {
            if self.round2_peer_packages.len() + 1 == self.max_signers as usize {
                println!("Received all round2 packages, entering part3");
                let part3_result = frost::keys::dkg::part3(
                    &r2_secret_package.clone(),
                    &self.round1_peer_packages,
                    &self.round2_peer_packages,
                );

                match part3_result {
                    Ok((private_key_package, pubkey_package)) => {
                        println!(
                            "!!!!!!! Public key: {:?}!!!!",
                            pubkey_package.verifying_key()
                        );

                        self.private_key_package = Some(private_key_package);
                        self.pubkey_package = Some(pubkey_package);
                    }
                    Err(e) => {
                        println!("DKG failed: {}", e);
                    }
                }
            }
        }
    }

    pub fn handle_input(&mut self, line: String, round1_topic: &IdentTopic) {
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

            self.handle_dkg_start(round1_topic);

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

    pub async fn main_loop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Read full lines from stdin
        let mut stdin = io::BufReader::new(io::stdin()).lines();

        let round1_topic = gossipsub::IdentTopic::new("round1_topic");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&round1_topic)?;

        let topic = gossipsub::IdentTopic::new("publish-key");
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

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

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    Identifier::derive(&bytes).unwrap()
}
