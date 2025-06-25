use rand::seq::SliceRandom;
use std::collections::BTreeMap;

use frost_secp256k1::rand_core::RngCore;
use frost_secp256k1::{self as frost};
use hex;
use libp2p::PeerId;
use tracing::{debug, error, info, warn};

use crate::handlers::signing::SigningState;
use crate::peer_id_to_identifier;
use crate::{
    NodeState, handlers::signing::ActiveSigning, handlers::withdrawl::SpendIntentState,
    wallet::Wallet,
};
use types::errors::NodeError;
use types::network::network_event::DirectMessage;
use types::network::network_protocol::Network;

impl SigningState {
    pub fn start_signing_session<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        message_hex: &str,
    ) -> Result<Option<u64>, NodeError> {
        if node.private_key_package.is_none() || node.pubkey_package.is_none() {
            error!("❌ DKG not completed – cannot start signing");
            return Err(NodeError::Error("DKG not completed".to_string()));
        }

        let Ok(message) = hex::decode(message_hex.trim()) else {
            error!("❌ Invalid hex message");
            return Err(NodeError::Error("Invalid hex message".to_string()));
        };
        if message.len() != 32 {
            error!(
                "❌ Message must be 32-byte (sighash) – got {} bytes",
                message.len()
            );
            return Err(NodeError::Error(
                "Message must be 32-byte (sighash)".to_string(),
            ));
        }

        info!("Starting signing session for message: {}", message_hex);

        if self.active_signing.is_some() {
            error!("❌ A signing session is already active");
            return Err(NodeError::Error(
                "A signing session is already active".to_string(),
            ));
        }

        let sign_id = node.rng.next_u64();
        let self_identifier = peer_id_to_identifier(&node.peer_id);

        // Select participants: self + first (min_signers -1) peers
        let required = (node
            .config
            .min_signers
            .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?
            - 1) as usize;
        if node.peers.len() < required {
            error!(
                "❌ Not enough peers – need at least {} others, have {}",
                required,
                node.peers.len()
            );
            error!("Available peers: {:?}", node.peers);
            return Err(NodeError::Error("Not enough peers".to_string()));
        }

        debug!(
            "✅ Starting signing with {} peers (need {})",
            node.peers.len(),
            required
        );
        debug!("Selected peers: {:?}", node.peers);

        // Randomly shuffle peers and pick required number
        let mut rng_rand = rand::rng();
        let mut peer_pool = node.peers.clone().into_iter().collect::<Vec<_>>();
        peer_pool.shuffle(&mut rng_rand);

        let selected_peers: Vec<PeerId> = peer_pool.into_iter().take(required).collect();

        // Generate nonces & commitments for self
        let key_pkg = match node.private_key_package.as_ref() {
            Some(key_pkg) => key_pkg.clone(),
            None => {
                return Err(NodeError::Error("No private key found".to_string()));
            }
        };
        let (nonces, commitments) = frost::round1::commit(key_pkg.signing_share(), &mut node.rng);

        let mut commitments_map = BTreeMap::new();
        commitments_map.insert(self_identifier, commitments);

        // Save active session
        self.active_signing = Some(ActiveSigning {
            sign_id,
            message: message.clone(),
            selected_peers: selected_peers.clone(),
            nonces,
            commitments: commitments_map,
            signature_shares: BTreeMap::new(),
            signing_package: None,
            is_coordinator: true,
        });

        // Broadcast SignRequest to chosen peers (skip self)
        for peer in &selected_peers {
            let req = DirectMessage::SignRequest {
                sign_id,
                message: message.clone(),
            };
            node.network_handle
                .send_private_message(*peer, req)
                .map_err(|e| NodeError::Error(format!("Failed to send private request: {e:?}")))?;
            debug!("🚀 Sent sign request to {}", peer);
        }

        Ok(Some(sign_id))
    }

    /// Handle incoming SignRequest (participant side)
    pub fn handle_sign_request<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        peer: PeerId,
        sign_id: u64,
        message: Vec<u8>,
    ) -> Result<(), NodeError> {
        if node.private_key_package.is_none() {
            let _ = node.network_handle.send_private_message(
                peer,
                DirectMessage::Commitments {
                    sign_id,
                    commitments: Vec::new(),
                },
            );
            return Ok(());
        }

        let key_pkg = match node.private_key_package.as_ref() {
            Some(key_pkg) => key_pkg.clone(),
            None => {
                return Err(NodeError::Error("No private key found".to_string()));
            }
        };
        let (nonces, commitments) = frost::round1::commit(key_pkg.signing_share(), &mut node.rng);

        // Save session (one at a time for simplicity)
        self.active_signing = Some(ActiveSigning {
            sign_id,
            message,
            selected_peers: Vec::new(),
            nonces,
            commitments: BTreeMap::new(), // not used for participant
            signature_shares: BTreeMap::new(),
            signing_package: None,
            is_coordinator: false,
        });

        let Ok(commit_bytes) = commitments.serialize() else {
            return Err(NodeError::Error(
                "Failed to serialize commitments".to_string(),
            ));
        };

        let resp = DirectMessage::Commitments {
            sign_id,
            commitments: commit_bytes,
        };
        let _ = node.network_handle.send_private_message(peer, resp);

        debug!(
            "🔐 Provided commitments for sign_id {} to {}",
            sign_id, peer
        );

        Ok(())
    }

    /// Coordinator receives commitments responses
    pub fn handle_commitments_response<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        peer: PeerId,
        sign_id: u64,
        commitments_bytes: &[u8],
    ) -> Result<(), NodeError> {
        let Some(active) = self.active_signing.as_mut() else {
            return Err(NodeError::Error("No active session".to_string()));
        };
        if !active.is_coordinator || active.sign_id != sign_id {
            return Err(NodeError::Error("Session id mismatch".to_string()));
        }

        let Ok(commitments) = frost::round1::SigningCommitments::deserialize(commitments_bytes)
        else {
            warn!("Failed to deserialize commitments from {}", peer);
            return Err(NodeError::Error(
                "Failed to deserialize commitments".to_string(),
            ));
        };
        let identifier = peer_id_to_identifier(&peer);
        active.commitments.insert(identifier, commitments);
        debug!(
            "📩 Received commitments from {} (total {}/{})",
            peer,
            active.commitments.len(),
            node.config
                .min_signers
                .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?
        );

        if active.commitments.len()
            == node
                .config
                .min_signers
                .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?
                as usize
        {
            // Build signing package
            let signing_package =
                frost::SigningPackage::new(active.commitments.clone(), &active.message);
            active.signing_package = Some(signing_package.clone());
            let Ok(pkg_bytes) = signing_package.serialize() else {
                warn!("Failed to serialize signing package");
                return Err(NodeError::Error(
                    "Failed to serialize signing package".to_string(),
                ));
            };

            // Send package to participants (excluding self)
            for peer in &active.selected_peers {
                let req = DirectMessage::SignPackage {
                    sign_id,
                    package: pkg_bytes.clone(),
                };
                let _ = node.network_handle.send_private_message(*peer, req);
            }

            // Generate our signature share
            let sig_share = frost::round2::sign(
                &signing_package,
                &active.nonces,
                match node.private_key_package.as_ref() {
                    Some(key_pkg) => key_pkg,
                    None => {
                        return Err(NodeError::Error("No private key found".to_string()));
                    }
                },
            );
            match sig_share {
                Ok(sig_share) => {
                    active
                        .signature_shares
                        .insert(peer_id_to_identifier(&node.peer_id), sig_share);
                }
                Err(e) => {
                    return Err(NodeError::Error(format!("Failed to sign: {e}")));
                }
            }

            debug!("📦 Distributed signing package for session {}", sign_id);
        }

        Ok(())
    }

    /// Participant handles SignPackage request
    pub fn handle_sign_package<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        peer: PeerId,
        sign_id: u64,
        package_bytes: &[u8],
    ) -> Result<(), NodeError> {
        let Some(active) = self.active_signing.as_ref() else {
            warn!("No active session to sign");
            return Err(NodeError::Error("No active session".to_string()));
        };
        if active.sign_id != sign_id {
            warn!("Session id mismatch");
            return Err(NodeError::Error("Session id mismatch".to_string()));
        }

        let Ok(signing_package) = frost::SigningPackage::deserialize(package_bytes) else {
            warn!("Failed to deserialize signing package");
            return Err(NodeError::Error(
                "Failed to deserialize signing package".to_string(),
            ));
        };

        let sig_share = frost::round2::sign(
            &signing_package,
            &active.nonces,
            match node.private_key_package.as_ref() {
                Some(key_pkg) => key_pkg,
                None => {
                    return Err(NodeError::Error("No private key found".to_string()));
                }
            },
        );
        match sig_share {
            Ok(sig_share) => {
                let sig_bytes = sig_share.serialize();
                let resp = DirectMessage::SignatureShare {
                    sign_id,
                    signature_share: sig_bytes,
                };
                let _ = node.network_handle.send_private_message(peer, resp);
            }
            Err(e) => {
                return Err(NodeError::Error(format!("Failed to sign: {e}")));
            }
        }

        debug!(
            "✍️  Sent signature share for session {} to {}",
            sign_id, peer
        );
        self.active_signing = None;

        Ok(())
    }

    /// Coordinator handles incoming signature share
    pub async fn handle_signature_share<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        peer: PeerId,
        sign_id: u64,
        sig_bytes: &[u8],
    ) -> Result<(), NodeError> {
        let Some(active) = self.active_signing.as_mut() else {
            return Err(NodeError::Error("No active session".to_string()));
        };
        if !active.is_coordinator || active.sign_id != sign_id {
            return Err(NodeError::Error("Session id mismatch".to_string()));
        }

        let Ok(sig_share) = frost::round2::SignatureShare::deserialize(sig_bytes) else {
            warn!("Failed to deserialize signature share from {}", peer);
            return Err(NodeError::Error(
                "Failed to deserialize signature share".to_string(),
            ));
        };
        let identifier = peer_id_to_identifier(&peer);
        active.signature_shares.insert(identifier, sig_share);
        debug!(
            "✅ Received signature share from {} (total {}/{})",
            peer,
            active.signature_shares.len(),
            node.config
                .min_signers
                .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?
        );

        if active.signature_shares.len()
            == node
                .config
                .min_signers
                .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?
                as usize
        {
            let signing_package = active
                .signing_package
                .clone()
                .ok_or_else(|| NodeError::Error("No signing package found".to_string()))?;
            let group_sig = frost::aggregate(
                &signing_package,
                &active.signature_shares,
                match node.pubkey_package.as_ref() {
                    Some(public_key) => public_key,
                    None => {
                        return Err(NodeError::Error("No public key found".to_string()));
                    }
                },
            )
            .expect("Aggregate");
            let sig_hex = hex::encode(group_sig.serialize().expect("serialize group sig"));
            debug!(
                "🎉 Final FROST signature for session {}: {}",
                sign_id, sig_hex
            );

            // If this signing session corresponds to a pending spend, finalise the transaction.
            if let Some(pending) = self.pending_spends.remove(&sign_id) {
                match Self::frost_signature_to_bitcoin(&group_sig) {
                    Ok(bitcoin_sig) => {
                        let mut tx = pending.tx;
                        let mut witness = bitcoin::witness::Witness::new();
                        witness.push(bitcoin_sig.as_ref());
                        if let Some(input) = tx.input.first_mut() {
                            input.witness = witness;
                        }
                        SpendIntentState::handle_signed_withdrawal(
                            node,
                            &tx,
                            pending.fee,
                            pending.user_pubkey,
                            pending.address_to,
                        )
                        .await?;
                        debug!("📤 Broadcasted transaction");
                    }
                    Err(e) => debug!("❌ Failed to convert signature: {}", e),
                }
            }
            // Reset
            self.active_signing = None;
        }

        Ok(())
    }
}
