use std::collections::BTreeMap;

use crate::swarm_manager::{PingBody, PrivateRequest, PrivateResponse};
use frost_secp256k1::Identifier;
use frost_secp256k1::keys::dkg::round2;
use frost_secp256k1::{self as frost, keys::dkg::round1};
use libp2p::gossipsub::IdentTopic;
use libp2p::{PeerId, gossipsub};

pub struct NodeState<'a> {
    pub r1_secret_package: Option<round1::SecretPackage>,
    pub peer_id: PeerId,
    pub part1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub part2_peer_packages: BTreeMap<Identifier, round2::Package>,
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
        peer_id: PeerId,
        swarm: &'a mut libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
        min_signers: u16,
        max_signers: u16,
    ) -> Self {
        NodeState {
            r1_secret_package: None,
            r2_secret_package: None,
            peer_id,
            part1_peer_packages: BTreeMap::new(),
            part2_peer_packages: BTreeMap::new(),
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

        // // Receive the message from self
        // self.handle_part1_payload(self.peer_id, round1_package);

        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(round1_topic.clone(), round1_package_bytes);

        println!(
            "Generated and published round1 package in response to DKG start signal {}",
            self.peer_id
        );
    }

    pub fn handle_part1_payload(&mut self, sender_peer_id: PeerId, package: round1::Package) {
        self.peers.push(sender_peer_id);
        // add package to peer packages
        self.part1_peer_packages
            .insert(peer_id_to_identifier(&sender_peer_id), package);

        println!(
            "Received round1 package from {} {}",
            sender_peer_id,
            self.part1_peer_packages.len()
        );
        if let Some(r1_secret_package) = self.r1_secret_package.as_ref() {
            if self.part1_peer_packages.len() + 1 == self.max_signers as usize {
                println!("Received all round1 packages, entering part2");
                // all packages received
                let part2_result =
                    frost::keys::dkg::part2(r1_secret_package.clone(), &self.part1_peer_packages);
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

    pub fn handle_part2_payload(
        &mut self,
        sender_peer_id: PeerId,
        package: round2::Package,
        response_channel: libp2p::request_response::ResponseChannel<PrivateResponse>,
    ) {
        println!(
            "Received round2 package from {} {}",
            sender_peer_id,
            self.part1_peer_packages.len()
        );

        // add package to peer packages
        self.part2_peer_packages
            .insert(peer_id_to_identifier(&sender_peer_id), package);

        let _ = self
            .swarm
            .behaviour_mut()
            .request_response
            .send_response(response_channel, PrivateResponse::Pong);

        if let Some(r2_secret_package) = self.r2_secret_package.as_ref() {
            if self.part2_peer_packages.len() + 1 == self.max_signers as usize {
                println!("Received all round2 packages, entering part3");
                let part3_result = frost::keys::dkg::part3(
                    &r2_secret_package.clone(),
                    &self.part1_peer_packages,
                    &self.part2_peer_packages,
                );

                match part3_result {
                    Ok((private_key_package, pubkey_package)) => {
                        println!("Successfully completed step 2");
                        println!("!!!!!!! Public key: {:?}!!!!", pubkey_package.verifying_key());

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
}

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    Identifier::derive(&bytes).unwrap()
}

pub fn handle_input(line: String, node_state: &mut NodeState<'_>, round1_topic: &IdentTopic) {
    if line.trim() == "/dkg" {
        // Create start-dkg topic
        let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");

        // Send a message to start DKG
        let start_message = format!("START_DKG:{}", node_state.peer_id);
        let _ = node_state
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(start_dkg_topic.clone(), start_message.as_bytes());

        node_state.handle_dkg_start(round1_topic);

        println!("Sent DKG start signal");
    } else if line.trim() == "/peers" {
        let connected_peers: Vec<_> = node_state
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
                    let direct_message =
                        format!("From {}: {}", node_state.peer_id, message_content);

                    let request_id = node_state
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(
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
