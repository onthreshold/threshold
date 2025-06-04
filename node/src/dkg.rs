use crate::{
    NodeState, peer_id_to_identifier,
    swarm_manager::{PrivateRequest, PrivateResponse},
};
use frost_secp256k1::{
    self as frost,
    keys::dkg::{round1, round2},
};
use libp2p::PeerId;

impl NodeState {
    pub fn handle_dkg_start(&mut self) {
        // Check if DKG keys already exist
        if self.private_key_package.is_some() && self.pubkey_package.is_some() {
            println!("DKG keys already exist, skipping DKG process");
            if let Some(pubkey) = &self.pubkey_package {
                println!("Existing public key: {:?}", pubkey.verifying_key());
            }
            return;
        }
        
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
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.start_dkg_topic.clone(), start_message.as_bytes());

        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.round1_topic.clone(), round1_package_bytes);

        self.try_enter_round2();

        println!(
            "Generated and published round1 package in response to DKG start signal from {}",
            self.peer_name(&self.peer_id)
        );
    }

    pub fn handle_round1_payload(&mut self, sender_peer_id: PeerId, package: round1::Package) {
        self.peers.push(sender_peer_id);
        // add package to peer packages
        self.round1_peer_packages
            .insert(peer_id_to_identifier(&sender_peer_id), package);

        println!(
            "Received round1 package from {} {}",
            self.peer_name(&sender_peer_id),
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
                        println!("-------------------- ENTERING ROUND 2 ---------------------");
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

                            println!("Sent round2 package to {}", self.peer_name(peer_to_send_to));
                        }
                    }
                    Err(e) => {
                        println!("DKG round2 failed: {}", e);
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
            self.peer_name(&sender_peer_id),
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
                        
                        // Save DKG keys to config file
                        if let Err(e) = self.save_dkg_keys() {
                            println!("Failed to save DKG keys: {}", e);
                        } else {
                            println!("DKG keys saved to config file");
                        }
                    }
                    Err(e) => {
                        println!("DKG failed: {}", e);
                    }
                }
            }
        }
    }
}
