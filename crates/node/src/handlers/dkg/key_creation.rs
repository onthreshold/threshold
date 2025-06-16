use frost_secp256k1::{self as frost, keys::dkg::round2};
use libp2p::PeerId;
use std::time::Duration;
use types::errors::NodeError;

use crate::{
    NodeState,
    db::Db,
    handlers::dkg::DkgState,
    peer_id_to_identifier,
    swarm_manager::{DirectMessage, Network},
    wallet::Wallet,
};

fn dkg_step_delay() -> Duration {
    std::env::var("DKG_STEP_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::ZERO)
}

impl DkgState {
    pub async fn handle_dkg_start<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
    ) -> Result<(), NodeError> {
        if self.dkg_started {
            tracing::debug!("DKG already started, skipping DKG process");
            return Ok(());
        }

        if node.private_key_package.is_some() && node.pubkey_package.is_some() {
            tracing::info!("DKG keys already exist, skipping DKG process");
            return Ok(());
        }

        if self.dkg_listeners.len() + 1 != node.max_signers as usize {
            tracing::debug!(
                "Not all listeners have subscribed to the DKG topic, not starting DKG process. Listeners: {:?}",
                self.dkg_listeners.len()
            );
            return Ok(());
        }

        tracing::info!("🚀 -------------------- STARTING DKG ---------------------------");

        self.dkg_started = true;

        tokio::time::sleep(dkg_step_delay()).await;

        // Run the DKG initialization code
        let participant_identifier = peer_id_to_identifier(&node.peer_id);

        let (round1_secret_package, round1_package) = frost::keys::dkg::part1(
            participant_identifier,
            node.max_signers,
            node.min_signers,
            node.rng,
        )
        .expect("Failed to generate round1 package");

        self.r1_secret_package = Some(round1_secret_package);

        let round1_package_bytes = round1_package
            .serialize()
            .expect("Failed to serialize round1 package");

        // Broadcast START_DKG message to the network,
        let start_message = format!("START_DKG:{}", node.peer_id);

        match node.network_handle.send_broadcast(
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

        match node
            .network_handle
            .send_broadcast(self.round1_topic.clone(), round1_package_bytes)
        {
            Ok(_) => tracing::debug!("Broadcast round1"),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send broadcast: {:?}",
                    e
                )));
            }
        }

        match self.try_enter_round2(node) {
            Ok(_) => {
                tracing::debug!(
                    "Generated and published round1 package in response to DKG start signal from {}",
                    &node.peer_id
                );
                Ok(())
            }
            Err(e) => Err(NodeError::Error(format!("Failed to enter round2: {}", e))),
        }
    }

    pub fn handle_round1_payload<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
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

        tracing::debug!(
            "Received round1 package from {} ({}/{})",
            sender_peer_id,
            self.round1_peer_packages.len(),
            node.max_signers - 1
        );

        self.try_enter_round2(node)?;

        Ok(())
    }

    pub fn try_enter_round2<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
    ) -> Result<(), NodeError> {
        if let Some(r1_secret_package) = self.r1_secret_package.as_ref() {
            if self.round1_peer_packages.len() + 1 == node.max_signers as usize {
                tracing::info!("Received all round1 packages, entering part2");
                // all packages received
                let part2_result =
                    frost::keys::dkg::part2(r1_secret_package.clone(), &self.round1_peer_packages);
                match part2_result {
                    Ok((round2_secret_package, round2_packages)) => {
                        tracing::info!(
                            "-------------------- ENTERING ROUND 2 ---------------------"
                        );
                        self.r1_secret_package = None;
                        self.r2_secret_package = Some(round2_secret_package);

                        for peer_to_send_to in self.dkg_listeners.iter() {
                            let identifier = peer_id_to_identifier(peer_to_send_to);
                            let package_to_send = match round2_packages.get(&identifier) {
                                Some(package) => package,
                                None => {
                                    tracing::warn!(
                                        "Round2 package not found for {}",
                                        peer_to_send_to
                                    );
                                    return Err(NodeError::Error(format!(
                                        "Round2 package not found for {}",
                                        peer_to_send_to
                                    )));
                                }
                            };

                            let request = DirectMessage::Round2Package(package_to_send.clone());

                            match node
                                .network_handle
                                .send_private_message(*peer_to_send_to, request)
                            {
                                Ok(_) => {
                                    tracing::debug!(
                                        "{} Sent round2 package to {}",
                                        node.peer_id,
                                        peer_to_send_to
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Round2 package not found for {}",
                                        peer_to_send_to
                                    );
                                    return Err(NodeError::Error(format!(
                                        "Failed to send private request: {:?}",
                                        e
                                    )));
                                }
                            }

                            tracing::debug!("Sent round2 package to {}", peer_to_send_to);
                        }

                        std::thread::sleep(dkg_step_delay());
                    }
                    Err(e) => {
                        return Err(NodeError::Error(format!("DKG round2 failed: {}", e)));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn handle_round2_payload<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
        sender_peer_id: PeerId,
        package: round2::Package,
    ) -> Result<(), NodeError> {
        let identifier = peer_id_to_identifier(&sender_peer_id);

        match node
            .network_handle
            .send_private_message(sender_peer_id, DirectMessage::Pong)
        {
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

        tracing::debug!(
            "Received round2 package from {} ({}/{})",
            sender_peer_id,
            self.round2_peer_packages.len(),
            node.max_signers - 1
        );
        if let Some(r2_secret_package) = self.r2_secret_package.as_ref() {
            if self.round2_peer_packages.len() + 1 == node.max_signers as usize {
                tracing::info!("Received all round2 packages, entering part3");
                std::thread::sleep(dkg_step_delay());

                let part3_result = frost::keys::dkg::part3(
                    &r2_secret_package.clone(),
                    &self.round1_peer_packages,
                    &self.round2_peer_packages,
                );

                match part3_result {
                    Ok((private_key_package, pubkey_package)) => {
                        tracing::info!(
                            "🏁 -------------------- DKG COMPLETED -------------------------"
                        );
                        tracing::info!(
                            "🎉 DKG finished successfully. Public key: {:?}",
                            pubkey_package.verifying_key()
                        );

                        self.save_dkg_keys(node, &private_key_package, &pubkey_package)?;

                        self.dkg_started = false;
                    }
                    Err(e) => {
                        tracing::error!("DKG failed during part3 aggregation: {}", e);
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
