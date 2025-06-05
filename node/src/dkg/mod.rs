use std::collections::{BTreeMap, HashSet};

use crate::{
    errors::NodeError,
    peer_id_to_identifier,
    swarm_manager::{NetworkHandle, PrivateRequest, PrivateResponse},
};
use frost_secp256k1::{
    self as frost, Identifier,
    keys::dkg::{round1, round2},
};
use libp2p::PeerId;

pub mod utils;

pub struct DkgState {
    pub dkg_started: bool,
    pub network_handle: NetworkHandle,
    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,

    pub peers_to_names: BTreeMap<PeerId, String>,
    pub dkg_listeners: HashSet<PeerId>,

    pub config_file: String,

    pub start_dkg_topic: libp2p::gossipsub::IdentTopic,
    pub round1_topic: libp2p::gossipsub::IdentTopic,

    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,

    pub r1_secret_package: Option<round1::SecretPackage>,
    pub r2_secret_package: Option<round2::SecretPackage>,

    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,
}

impl DkgState {
    pub fn handle_dkg_start(&mut self) -> Result<(), NodeError> {
        if self.dkg_started {
            println!("DKG already started, skipping DKG process");
            return Ok(());
        }

        // Check if DKG keys already exist
        if self.private_key_package.is_some() && self.pubkey_package.is_some() {
            println!("DKG keys already exist, skipping DKG process");
            if let Some(pubkey) = &self.pubkey_package {
                println!("Existing public key: {:?}", pubkey.verifying_key());
            }
            return Ok(());
        }

        if self.dkg_listeners.len() + 1 != self.max_signers as usize {
            println!(
                "Not all listeners have subscribed to the DKG topic, not starting DKG process. Listeners: {:?}",
                self.dkg_listeners.len()
            );
            return Ok(());
        }

        self.dkg_started = true;

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

        // Broadcast START_DKG message to the network,
        let start_message = format!("START_DKG:{}", self.peer_id);

        match self.network_handle.send_broadcast(
            self.start_dkg_topic.clone(),
            start_message.as_bytes().to_vec(),
        ) {
            Ok(_) => (),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send broadcast: {:?}",
                    e
                )));
            }
        }

        match self
            .network_handle
            .send_broadcast(self.round1_topic.clone(), round1_package_bytes)
        {
            Ok(_) => (),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send broadcast: {:?}",
                    e
                )));
            }
        }

        match self.try_enter_round2() {
            Ok(_) => {
                println!(
                    "Generated and published round1 package in response to DKG start signal from {}",
                    &self.peer_id
                );
                Ok(())
            }
            Err(e) => {
                Err(NodeError::Error(format!("Failed to enter round2: {}", e)))
            }
        }
    }

    pub fn handle_round1_payload(
        &mut self,
        sender_peer_id: PeerId,
        package: Vec<u8>,
    ) -> Result<(), NodeError> {
        let identifier = peer_id_to_identifier(&sender_peer_id);
        let package = match frost::keys::dkg::round1::Package::deserialize(&package) {
            Ok(package) => package,
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to deserialize round1 package: {}",
                    e
                )));
            }
        };
        // Add package to peer packages
        self.round1_peer_packages.insert(identifier, package);

        println!(
            "Received round1 package from {} ({}/{})",
            sender_peer_id,
            self.round1_peer_packages.len(),
            self.max_signers - 1
        );

        self.try_enter_round2()?;

        Ok(())
    }

    pub fn try_enter_round2(&mut self) -> Result<(), NodeError> {
        if let Some(r1_secret_package) = self.r1_secret_package.as_ref() {
            if self.round1_peer_packages.len() + 1 == self.max_signers as usize {
                println!("Received all round1 packages, entering part2");
                // all packages received
                let part2_result =
                    frost::keys::dkg::part2(r1_secret_package.clone(), &self.round1_peer_packages);
                match part2_result {
                    Ok((round2_secret_package, round2_packages)) => {
                        println!("-------------------- ENTERING ROUND 2 ---------------------");
                        self.r1_secret_package = None;
                        self.r2_secret_package = Some(round2_secret_package);

                        for peer_to_send_to in self.peers.iter() {
                            let identifier = peer_id_to_identifier(peer_to_send_to);
                            let package_to_send = match round2_packages.get(&identifier) {
                                Some(package) => package,
                                None => {
                                    return Err(NodeError::Error(format!(
                                        "Round2 package not found for {}",
                                        peer_to_send_to
                                    )));
                                }
                            };

                            let request = PrivateRequest::Round2Package(package_to_send.clone());

                            match self
                                .network_handle
                                .send_private_request(*peer_to_send_to, request)
                            {
                                Ok(_) => (),
                                Err(e) => {
                                    return Err(NodeError::Error(format!(
                                        "Failed to send private request: {:?}",
                                        e
                                    )));
                                }
                            }

                            println!("Sent round2 package to {}", peer_to_send_to);
                        }
                    }
                    Err(e) => {
                        return Err(NodeError::Error(format!("DKG round2 failed: {}", e)));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn handle_round2_payload(
        &mut self,
        sender_peer_id: PeerId,
        package: round2::Package,
        response_channel: libp2p::request_response::ResponseChannel<PrivateResponse>,
    ) -> Result<(), NodeError> {
        let identifier = peer_id_to_identifier(&sender_peer_id);

        // Skip duplicate packages
        if self.round2_peer_packages.contains_key(&identifier) {
            println!(
                "Duplicate round2 package from {} – already recorded",
                sender_peer_id
            );
            match self
                .network_handle
                .send_private_response(response_channel, PrivateResponse::Pong)
            {
                Ok(_) => (),
                Err(e) => {
                    return Err(NodeError::Error(format!(
                        "Failed to send private response: {:?}",
                        e
                    )));
                }
            }

            return Ok(());
        }

        // Add package to peer packages
        self.round2_peer_packages.insert(identifier, package);

        println!(
            "Received round2 package from {} ({}/{})",
            sender_peer_id,
            self.round2_peer_packages.len(),
            self.max_signers - 1
        );

        // Ack the received package
        match self
            .network_handle
            .send_private_response(response_channel, PrivateResponse::Pong)
        {
            Ok(_) => (),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send private response: {:?}",
                    e
                )));
            }
        }

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
                            "🎉 DKG finished successfully. Public key: {:?}",
                            pubkey_package.verifying_key()
                        );

                        self.private_key_package = Some(private_key_package);
                        self.pubkey_package = Some(pubkey_package);

                        if let Err(e) = self.save_dkg_keys() {
                            println!("Failed to save DKG keys: {}", e);
                        } else {
                            println!("DKG keys saved to config file");
                        }

                        self.dkg_started = false;
                    }
                    Err(e) => {
                        println!("DKG failed during part3 aggregation: {}", e);
                        // Reset state so that a fresh DKG can be attempted again later
                        self.reset_dkg_state();
                    }
                }
            }
        }

        Ok(())
    }

    /// Reset DKG state after a failed run so that a new DKG round can be initiated.
    fn reset_dkg_state(&mut self) {
        self.dkg_started = false;
        self.r1_secret_package = None;
        self.r2_secret_package = None;
        self.round1_peer_packages.clear();
        self.round2_peer_packages.clear();
    }
}
