use frost_secp256k1::{self as frost, keys::dkg::round2};
use libp2p::PeerId;
use tracing::{debug, error, info, warn};
use types::errors::NodeError;

use crate::{
    dkg::DkgState,
    peer_id_to_identifier,
    swarm_manager::{DirectMessage, Network, SelfRequest},
};

impl DkgState {
    pub async fn handle_dkg_start(
        &mut self,
        network_handle: &impl Network,
    ) -> Result<(), NodeError> {
        if self.dkg_started {
            debug!("DKG already started, skipping DKG process");
            return Ok(());
        }

        // Check if DKG keys already exist
        if self.dkg_completed {
            info!("DKG keys already exist, skipping DKG process");
            return Ok(());
        }

        if self.dkg_listeners.len() + 1 != self.max_signers as usize {
            debug!(
                "Not all listeners have subscribed to the DKG topic, not starting DKG process. Listeners: {:?}",
                self.dkg_listeners.len()
            );
            return Ok(());
        }

        info!("Starting DKG process");

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

        match network_handle.send_broadcast(
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

        match network_handle.send_broadcast(self.round1_topic.clone(), round1_package_bytes) {
            Ok(_) => (),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send broadcast: {:?}",
                    e
                )));
            }
        }

        match self.try_enter_round2(network_handle) {
            Ok(_) => {
                info!(
                    "Generated and published round1 package in response to DKG start signal from {}",
                    &self.peer_id
                );
                Ok(())
            }
            Err(e) => Err(NodeError::Error(format!("Failed to enter round2: {}", e))),
        }
    }

    pub fn handle_round1_payload(
        &mut self,
        network_handle: &impl Network,
        sender_peer_id: PeerId,
        package: &[u8],
    ) -> Result<(), NodeError> {
        let identifier = peer_id_to_identifier(&sender_peer_id);
        let package = match frost::keys::dkg::round1::Package::deserialize(package) {
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

        debug!(
            "Received round1 package from {} ({}/{})",
            sender_peer_id,
            self.round1_peer_packages.len(),
            self.max_signers - 1
        );

        self.try_enter_round2(network_handle)?;

        Ok(())
    }

    pub fn try_enter_round2(&mut self, network_handle: &impl Network) -> Result<(), NodeError> {
        if let Some(r1_secret_package) = self.r1_secret_package.as_ref() {
            if self.round1_peer_packages.len() + 1 == self.max_signers as usize {
                info!("Received all round1 packages, entering part2");
                // all packages received
                let part2_result =
                    frost::keys::dkg::part2(r1_secret_package.clone(), &self.round1_peer_packages);
                match part2_result {
                    Ok((round2_secret_package, round2_packages)) => {
                        info!("-------------------- ENTERING ROUND 2 ---------------------");
                        self.r1_secret_package = None;
                        self.r2_secret_package = Some(round2_secret_package);

                        for peer_to_send_to in self.dkg_listeners.iter() {
                            let identifier = peer_id_to_identifier(peer_to_send_to);
                            let package_to_send = match round2_packages.get(&identifier) {
                                Some(package) => package,
                                None => {
                                    warn!("Round2 package not found for {}", peer_to_send_to);
                                    return Err(NodeError::Error(format!(
                                        "Round2 package not found for {}",
                                        peer_to_send_to
                                    )));
                                }
                            };

                            let request = DirectMessage::Round2Package(package_to_send.clone());

                            match network_handle.send_private_message(*peer_to_send_to, request) {
                                Ok(_) => (),
                                Err(e) => {
                                    error!("Round2 package not found for {}", peer_to_send_to);
                                    return Err(NodeError::Error(format!(
                                        "Failed to send private request: {:?}",
                                        e
                                    )));
                                }
                            }

                            debug!("Sent round2 package to {}", peer_to_send_to);
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
        network_handle: &impl Network,
        sender_peer_id: PeerId,
        package: round2::Package,
    ) -> Result<(), NodeError> {
        let identifier = peer_id_to_identifier(&sender_peer_id);

        match network_handle.send_private_message(sender_peer_id, DirectMessage::Pong) {
            Ok(_) => (),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send private response: {:?}",
                    e
                )));
            }
        }

        // Add package to peer packages
        self.round2_peer_packages.insert(identifier, package);

        debug!(
            "Received round2 package from {} ({}/{})",
            sender_peer_id,
            self.round2_peer_packages.len(),
            self.max_signers - 1
        );

        if let Some(r2_secret_package) = self.r2_secret_package.as_ref() {
            if self.round2_peer_packages.len() + 1 == self.max_signers as usize {
                info!("Received all round2 packages, entering part3");
                let part3_result = frost::keys::dkg::part3(
                    &r2_secret_package.clone(),
                    &self.round1_peer_packages,
                    &self.round2_peer_packages,
                );

                match part3_result {
                    Ok((private_key_package, pubkey_package)) => {
                        info!(
                            "🎉 DKG finished successfully. Public key: {:?}",
                            pubkey_package.verifying_key()
                        );

                        network_handle
                            .send_self_request(
                                SelfRequest::SetFrostKeys {
                                    private_key: private_key_package.serialize().map_err(|e| {
                                        NodeError::Error(format!(
                                            "Failed to serialize private key: {}",
                                            e
                                        ))
                                    })?,
                                    public_key: pubkey_package.serialize().map_err(|e| {
                                        NodeError::Error(format!(
                                            "Failed to serialize public key: {}",
                                            e
                                        ))
                                    })?,
                                },
                                false,
                            )
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send self request: {:?}", e))
                            })?;

                        self.dkg_started = false;
                    }
                    Err(e) => {
                        error!("DKG failed during part3 aggregation: {}", e);
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
