use std::collections::BTreeMap;

use frost_secp256k1::rand_core::RngCore;
use frost_secp256k1::{self as frost};
use hex;
use libp2p::{PeerId, request_response};
use rand::seq::SliceRandom;

use crate::node::{ActiveSigning, NodeState, peer_id_to_identifier};
use crate::swarm_manager::{PrivateRequest, PrivateResponse};

impl<'a> NodeState<'a> {
    /// Coordinator entrypoint. Start a threshold signing session across the network.
    /// `message_hex` must be hex-encoded 32-byte sighash.
    pub fn start_signing_session(&mut self, message_hex: &str) {
        if self.private_key_package.is_none() || self.pubkey_package.is_none() {
            println!("❌ DKG not completed – cannot start signing");
            return;
        }

        let Ok(message) = hex::decode(message_hex.trim()) else {
            println!("❌ Invalid hex message");
            return;
        };
        if message.len() != 32 {
            println!(
                "❌ Message must be 32-byte (sighash) – got {} bytes",
                message.len()
            );
            return;
        }

        // Only allow one active session for simplicity
        if self.active_signing.is_some() {
            println!("❌ A signing session is already active");
            return;
        }

        let sign_id = self.rng.next_u64();
        let self_identifier = peer_id_to_identifier(&self.peer_id);

        // Select participants: self + first (min_signers -1) peers
        let required = (self.min_signers - 1) as usize;
        if self.peers.len() < required {
            println!("❌ Not enough peers – need at least {} others", required);
            return;
        }
        // Randomly shuffle peers and pick required number
        let mut rng_rand = rand::rng();
        let mut peer_pool = self.peers.clone();
        peer_pool.shuffle(&mut rng_rand);

        let selected_peers: Vec<PeerId> = peer_pool.into_iter().take(required).collect();

        let mut participants: Vec<_> = Vec::new();
        participants.push(self_identifier);
        for peer in &selected_peers {
            participants.push(peer_id_to_identifier(peer));
        }

        // Generate nonces & commitments for self
        let key_pkg = self.private_key_package.as_ref().unwrap();
        let (nonces, commitments) = frost::round1::commit(key_pkg.signing_share(), &mut self.rng);

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
            let req = PrivateRequest::SignRequest {
                sign_id,
                message: message.clone(),
            };
            let _id = self
                .swarm
                .behaviour_mut()
                .request_response
                .send_request(peer, req);
        }

        println!(
            "🚀 Started signing session {} with {} participants",
            sign_id, self.min_signers
        );
    }

    /// Handle incoming SignRequest (participant side)
    pub fn handle_sign_request(
        &mut self,
        peer: PeerId,
        sign_id: u64,
        message: Vec<u8>,
        channel: request_response::ResponseChannel<PrivateResponse>,
    ) {
        if self.private_key_package.is_none() {
            let _ = self.swarm.behaviour_mut().request_response.send_response(
                channel,
                PrivateResponse::Commitments {
                    sign_id,
                    commitments: Vec::new(),
                },
            );
            return;
        }

        let key_pkg = self.private_key_package.as_ref().unwrap();
        let (nonces, commitments) = frost::round1::commit(key_pkg.signing_share(), &mut self.rng);

        // Save session (one at a time for simplicity)
        self.active_signing = Some(ActiveSigning {
            sign_id,
            message: message.clone(),
            selected_peers: Vec::new(),
            nonces,
            commitments: BTreeMap::new(), // not used for participant
            signature_shares: BTreeMap::new(),
            signing_package: None,
            is_coordinator: false,
        });

        let Ok(commit_bytes) = commitments.serialize() else {
            println!("Failed to serialize commitments");
            return;
        };

        let resp = PrivateResponse::Commitments {
            sign_id,
            commitments: commit_bytes,
        };
        let _ = self
            .swarm
            .behaviour_mut()
            .request_response
            .send_response(channel, resp);

        println!(
            "🔐 Provided commitments for sign_id {} to {}",
            sign_id, peer
        );
    }

    /// Coordinator receives commitments responses
    pub fn handle_commitments_response(
        &mut self,
        peer: PeerId,
        sign_id: u64,
        commitments_bytes: Vec<u8>,
    ) {
        let Some(active) = self.active_signing.as_mut() else {
            return;
        };
        if !active.is_coordinator || active.sign_id != sign_id {
            return;
        }

        let Ok(commitments) = frost::round1::SigningCommitments::deserialize(&commitments_bytes)
        else {
            println!("Failed to deserialize commitments from {}", peer);
            return;
        };
        let identifier = peer_id_to_identifier(&peer);
        active.commitments.insert(identifier, commitments);
        println!(
            "📩 Received commitments from {} (total {}/{})",
            peer,
            active.commitments.len(),
            self.min_signers
        );

        if active.commitments.len() == self.min_signers as usize {
            // Build signing package
            let signing_package =
                frost::SigningPackage::new(active.commitments.clone(), &active.message);
            active.signing_package = Some(signing_package.clone());
            let Ok(pkg_bytes) = signing_package.serialize() else {
                println!("Failed to serialize signing package");
                return;
            };

            // Send package to participants (excluding self)
            for peer in &active.selected_peers {
                let req = PrivateRequest::SignPackage {
                    sign_id,
                    package: pkg_bytes.clone(),
                };
                let _ = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(peer, req);
            }

            // Generate our signature share
            let sig_share = frost::round2::sign(
                &signing_package,
                &active.nonces,
                self.private_key_package.as_ref().unwrap(),
            )
            .expect("Signing share");
            active
                .signature_shares
                .insert(peer_id_to_identifier(&self.peer_id), sig_share);

            println!("📦 Distributed signing package for session {}", sign_id);
        }
    }

    /// Participant handles SignPackage request
    pub fn handle_sign_package(
        &mut self,
        peer: PeerId,
        sign_id: u64,
        package_bytes: Vec<u8>,
        channel: request_response::ResponseChannel<PrivateResponse>,
    ) {
        let Some(active) = self.active_signing.as_ref() else {
            println!("No active session to sign");
            return;
        };
        if active.sign_id != sign_id {
            println!("Session id mismatch");
            return;
        }

        let Ok(signing_package) = frost::SigningPackage::deserialize(&package_bytes) else {
            println!("Failed to deserialize signing package");
            return;
        };

        let sig_share = frost::round2::sign(
            &signing_package,
            &active.nonces,
            self.private_key_package.as_ref().unwrap(),
        )
        .expect("Sign share");

        let sig_bytes = sig_share.serialize();
        let resp = PrivateResponse::SignatureShare {
            sign_id,
            signature_share: sig_bytes,
        };
        let _ = self
            .swarm
            .behaviour_mut()
            .request_response
            .send_response(channel, resp);
        println!(
            "✍️  Sent signature share for session {} to {}",
            sign_id, peer
        );
    }

    /// Coordinator handles incoming signature share
    pub fn handle_signature_share(&mut self, peer: PeerId, sign_id: u64, sig_bytes: Vec<u8>) {
        let Some(active) = self.active_signing.as_mut() else {
            return;
        };
        if !active.is_coordinator || active.sign_id != sign_id {
            return;
        }

        let Ok(sig_share) = frost::round2::SignatureShare::deserialize(&sig_bytes) else {
            println!("Failed to deserialize signature share from {}", peer);
            return;
        };
        let identifier = peer_id_to_identifier(&peer);
        active.signature_shares.insert(identifier, sig_share);
        println!(
            "✅ Received signature share from {} (total {}/{})",
            peer,
            active.signature_shares.len(),
            self.min_signers
        );

        if active.signature_shares.len() == self.min_signers as usize {
            let signing_package = active.signing_package.clone().unwrap();
            let group_sig = frost::aggregate(
                &signing_package,
                &active.signature_shares,
                self.pubkey_package.as_ref().unwrap(),
            )
            .expect("Aggregate");
            let sig_hex = hex::encode(group_sig.serialize().expect("serialize group sig"));
            println!(
                "🎉 Final FROST signature for session {}: {}",
                sign_id, sig_hex
            );
            // Reset
            self.active_signing = None;
        }
    }
}
