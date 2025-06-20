use abci::{ChainMessage, ChainResponse};
use frost_secp256k1::{self as frost, keys::dkg::round2};
use libp2p::PeerId;
use prost::Message as ProstMessage;
use protocol::block::{ChainConfig, ValidatorInfo};
use std::time::Duration;
use types::{errors::NodeError, network::network_event::DirectMessage};

use crate::peer_id_to_identifier;
use crate::{NodeState, handlers::dkg::DkgState, wallet::Wallet};
use types::network::network_protocol::Network;
use types::proto::p2p_proto::{
    DkgMessage, GossipsubMessage, StartDkgMessage, dkg_message::Message,
};

fn decode_gossipsub_dkg_message(
    data: &[u8],
) -> Result<types::proto::p2p_proto::DkgMessage, String> {
    let gossipsub_msg = <types::proto::p2p_proto::GossipsubMessage as ProstMessage>::decode(data)
        .map_err(|e| format!("Failed to decode GossipsubMessage: {e}"))?;

    if let Some(types::proto::p2p_proto::gossipsub_message::Message::Dkg(dkg_msg)) =
        gossipsub_msg.message
    {
        Ok(dkg_msg)
    } else {
        Err("Expected DKG message in GossipsubMessage".to_string())
    }
}

fn dkg_step_delay() -> Duration {
    std::env::var("DKG_STEP_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or_else(|| Duration::from_secs(1), Duration::from_secs)
}

impl DkgState {
    pub fn handle_dkg_start<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
    ) -> Result<(), NodeError> {
        if self.dkg_started {
            tracing::debug!("DKG already started, skipping DKG process");
            return Ok(());
        }

        if node.private_key_package.is_some() && node.pubkey_package.is_some() {
            tracing::info!("DKG keys already exist, skipping DKG process");
            return Ok(());
        }

        if self.dkg_listeners.len() + 1
            != node
                .config
                .max_signers
                .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?
                as usize
            && self.round1_listeners.len() + 1
                == node
                    .config
                    .max_signers
                    .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?
                    as usize
        {
            tracing::debug!(
                "Not all listeners have subscribed to the DKG topic, not starting DKG process. Listeners: {:?}",
                self.dkg_listeners.len()
            );
            return Ok(());
        }

        tracing::info!("🚀 -------------------- Finally starting DKG ---------------------------");

        self.dkg_started = true;

        tracing::info!(
            "🚀 -------------------- Sleeping for DKG step delay ---------------------------"
        );

        // tokio::time::sleep(dkg_step_delay()).await;

        tracing::info!(
            "🚀 -------------------- Generating round1 package ---------------------------"
        );
        // Run the DKG initialization code
        let participant_identifier = peer_id_to_identifier(&node.peer_id);

        tracing::info!(
            "🚀 -------------------- Generating round1 package YESSS ---------------------------"
        );

        let (round1_secret_package, round1_package) = frost::keys::dkg::part1(
            participant_identifier,
            node.config
                .max_signers
                .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?,
            node.config
                .min_signers
                .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?,
            node.rng,
        )
        .expect("Failed to generate round1 package");

        self.r1_secret_package = Some(round1_secret_package);

        // Broadcast START_DKG message to the network using protobuf

        tracing::info!(
            "🚀 -------------------- Broadcasting START_DKG message ---------------------------"
        );
        let start_dkg_message = DkgMessage {
            message: Some(Message::StartDkg(StartDkgMessage {
                peer_id: node.peer_id.to_string(),
            })),
        };

        let gossipsub_message = GossipsubMessage {
            message: Some(types::proto::p2p_proto::gossipsub_message::Message::Dkg(
                start_dkg_message,
            )),
        };

        match node
            .network_handle
            .send_broadcast(self.start_dkg_topic.clone(), gossipsub_message)
        {
            Ok(()) => (),
            Err(e) => {
                return Err(NodeError::Error(format!("Failed to send broadcast: {e:?}")));
            }
        }

        tracing::info!(
            "🚀 -------------------- Broadcasting round1 package ---------------------------"
        );
        // Broadcast round1 package using protobuf
        let serialized_pkg = round1_package
            .serialize()
            .map_err(|e| NodeError::Error(format!("Failed to serialize round1 package: {e}")))?;

        let round1_dkg_message = DkgMessage {
            message: Some(Message::Round1Package(
                types::proto::p2p_proto::Round1Package {
                    package_data: serialized_pkg,
                },
            )),
        };

        let round1_gossipsub_message = GossipsubMessage {
            message: Some(types::proto::p2p_proto::gossipsub_message::Message::Dkg(
                round1_dkg_message,
            )),
        };

        match node
            .network_handle
            .send_broadcast(self.round1_topic.clone(), round1_gossipsub_message)
        {
            Ok(()) => tracing::info!("Broadcast round1"),
            Err(e) => {
                return Err(NodeError::Error(format!("Failed to send broadcast: {e:?}")));
            }
        }

        match self.try_enter_round2(node) {
            Ok(()) => {
                tracing::debug!(
                    "Generated and published round1 package in response to DKG start signal from {}",
                    &node.peer_id
                );
                Ok(())
            }
            Err(e) => Err(NodeError::Error(format!("Failed to enter round2: {e}"))),
        }
    }

    pub fn handle_round1_payload<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        sender_peer_id: PeerId,
        protobuf_data: &[u8],
    ) -> Result<(), NodeError> {
        // Decode the protobuf message first
        let dkg_message = decode_gossipsub_dkg_message(protobuf_data)
            .map_err(|e| NodeError::Error(format!("Failed to decode DKG message: {e}")))?;

        // Extract the round1 package from the protobuf message
        let Some(types::proto::p2p_proto::dkg_message::Message::Round1Package(round1_pkg)) =
            dkg_message.message
        else {
            return Err(NodeError::Error(
                "Expected Round1Package in DKG message".to_string(),
            ));
        };

        let identifier = peer_id_to_identifier(&sender_peer_id);
        let package = match frost::keys::dkg::round1::Package::deserialize(&round1_pkg.package_data)
        {
            Ok(package) => package,
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to deserialize round1 package: {e}"
                )));
            }
        };
        // Add package to peer packages
        self.round1_peer_packages.insert(identifier, package);

        tracing::info!(
            "Received round1 package from {} ({}/{})",
            node.network_handle.peer_name(&sender_peer_id),
            self.round1_peer_packages.len(),
            node.config
                .max_signers
                .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?
                - 1
        );

        self.try_enter_round2(node)?;

        Ok(())
    }

    pub fn try_enter_round2<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
    ) -> Result<(), NodeError> {
        if let Some(r1_secret_package) = self.r1_secret_package.as_ref() {
            if self.round1_peer_packages.len() + 1
                == node
                    .config
                    .max_signers
                    .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?
                    as usize
            {
                tracing::info!("Received all round1 packages, entering part2");
                // all packages received
                let part2_result =
                    frost::keys::dkg::part2(r1_secret_package.clone(), &self.round1_peer_packages);
                match part2_result {
                    Ok((round2_secret_package, round2_packages)) => {
                        tracing::info!("✅ All round1 packages collected; entering FROST round 2");
                        self.r1_secret_package = None;
                        self.r2_secret_package = Some(round2_secret_package);

                        for peer_to_send_to in &self.dkg_listeners {
                            let identifier = peer_id_to_identifier(peer_to_send_to);
                            let package_to_send =
                                round2_packages.get(&identifier).ok_or_else(|| {
                                    tracing::warn!(
                                        "Round2 package not found for {}",
                                        peer_to_send_to
                                    );
                                    NodeError::Error(format!(
                                        "Round2 package not found for {peer_to_send_to}"
                                    ))
                                })?;

                            let request = DirectMessage::Round2Package(package_to_send.clone());

                            match node
                                .network_handle
                                .send_private_message(*peer_to_send_to, request)
                            {
                                Ok(()) => {
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
                                        "Failed to send private request: {e:?}"
                                    )));
                                }
                            }

                            tracing::debug!("Sent round2 package to {}", peer_to_send_to);
                        }

                        std::thread::sleep(dkg_step_delay());
                    }
                    Err(e) => {
                        return Err(NodeError::Error(format!("DKG round2 failed: {e}")));
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn handle_round2_payload<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        sender_peer_id: PeerId,
        package: round2::Package,
    ) -> Result<(), NodeError> {
        let identifier = peer_id_to_identifier(&sender_peer_id);

        match node
            .network_handle
            .send_private_message(sender_peer_id, DirectMessage::Pong)
        {
            Ok(()) => (),
            Err(e) => {
                return Err(NodeError::Error(format!(
                    "Failed to send private response: {e:?}"
                )));
            }
        }

        // Add package to peer packages
        self.round2_peer_packages.insert(identifier, package);

        tracing::info!(
            "Received round2 package from {} ({}/{})",
            node.network_handle.peer_name(&sender_peer_id),
            self.round2_peer_packages.len(),
            node.config
                .max_signers
                .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?
                - 1
        );
        if let Some(r2_secret_package) = self.r2_secret_package.as_ref() {
            if self.round2_peer_packages.len() + 1
                == node
                    .config
                    .max_signers
                    .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?
                    as usize
            {
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

                        node.pubkey_package = Some(pubkey_package.clone());
                        node.private_key_package = Some(private_key_package.clone());

                        node.config
                            .save_dkg_keys(&private_key_package, &pubkey_package)?;

                        let mut validators: Vec<ValidatorInfo> = node
                            .peers
                            .iter()
                            .map(|peer_id| ValidatorInfo {
                                pub_key: peer_id.to_bytes(),
                                stake: 100,
                            })
                            .collect();

                        validators.sort_by(|a, b| a.pub_key.cmp(&b.pub_key));

                        let chain_config = ChainConfig {
                            block_time_seconds: 10,
                            min_signers: node.config.min_signers.ok_or_else(|| {
                                NodeError::Error("Min signers not set".to_string())
                            })?,
                            max_signers: node.config.max_signers.ok_or_else(|| {
                                NodeError::Error("Max signers not set".to_string())
                            })?,
                            min_stake: 100,
                            max_block_size: 1000,
                        };

                        let ChainResponse::CreateGenesisBlock { error: None } = node
                            .chain_interface_tx
                            .send_message_with_response(ChainMessage::CreateGenesisBlock {
                                validators,
                                chain_config,
                                pubkey: pubkey_package.clone(),
                            })
                            .await?
                        else {
                            return Err(NodeError::Error(
                                "Failed to create genesis block".to_string(),
                            ));
                        };

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
