use frost_secp256k1::{self as frost, Identifier};
use libp2p::{PeerId, identity::Keypair};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use swarm_manager::{NetworkHandle, SwarmManager};

use crate::{dkg::DkgState, swarm_manager::build_swarm};

pub mod dkg;
pub mod grpc_handler;
pub mod grpc_operator;
pub mod main_loop;
pub mod signing;
pub mod swarm_manager;
pub mod wallet;

pub mod errors;

#[derive(Serialize, Deserialize, PartialEq)]
pub struct PeerData {
    pub name: String,
    pub public_key: String,
}

#[derive(Serialize, Deserialize)]
pub struct DkgKeys {
    pub encrypted_private_key_package_b64: String,
    pub dkg_encryption_params: EncryptionParams,
    pub pubkey_package_b64: String,
}

#[derive(Serialize, Deserialize)]
pub struct EncryptionParams {
    pub kdf: String,
    pub salt_b64: String,
    pub iv_b64: String,
}

#[derive(Serialize, Deserialize)]
pub struct KeyData {
    pub public_key_b58: String,
    pub encrypted_private_key_b64: String,
    pub encryption_params: EncryptionParams,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub allowed_peers: Vec<PeerData>,
    pub key_data: KeyData,
    pub dkg_keys: Option<DkgKeys>,
}

pub struct NodeState {
    pub allowed_peers: Vec<PeerId>,
    pub peers_to_names: BTreeMap<PeerId, String>,

    // DKG
    pub dkg_state: DkgState,

    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,
    pub swarm: SwarmManager,

    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,

    // FROST signing
    pub active_signing: Option<ActiveSigning>,
    pub wallet: crate::wallet::SimpleWallet,
    pub pending_spends: std::collections::BTreeMap<u64, crate::wallet::PendingSpend>,

    // Config management
    pub config_file: String,

    pub network_handle: NetworkHandle,
}

impl NodeState {
    pub fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .unwrap_or(&peer_id.to_string())
            .clone()
    }

    pub fn new_from_config(
        keypair: Keypair,
        peer_data: Vec<PeerData>,
        min_signers: u16,
        max_signers: u16,
        config_file: String,
    ) -> Self {
        // Node State
        let (network_handle, swarm) = build_swarm(keypair.clone()).expect("Failed to build swarm");
        let peer_id = *swarm.inner.local_peer_id();

        let allowed_peers: Vec<PeerId> = peer_data
            .iter()
            .map(|peer| peer.public_key.parse().unwrap())
            .collect();

        let peers_to_names: BTreeMap<PeerId, String> = peer_data
            .iter()
            .map(|peer| (peer.public_key.parse().unwrap(), peer.name.clone()))
            .collect();

        NodeState {
            network_handle: network_handle.clone(),
            allowed_peers: allowed_peers.clone(),
            peers_to_names: peers_to_names.clone(),
            peer_id,
            swarm,
            min_signers,
            max_signers,
            dkg_state: DkgState::new(
                network_handle.clone(),
                min_signers,
                max_signers,
                peer_id,
                peers_to_names,
                config_file.clone(),
            ),
            peers: HashSet::new(),
            rng: frost::rand_core::OsRng,
            active_signing: None,
            wallet: crate::wallet::SimpleWallet::new(),
            pending_spends: BTreeMap::new(),
            config_file: config_file.clone(),
        }
    }

    // Keep the old new() for backwards compatibility
    pub fn new(
        keypair: Keypair,
        peer_data: Vec<PeerData>,
        min_signers: u16,
        max_signers: u16,
    ) -> Self {
        Self::new_from_config(
            keypair,
            peer_data,
            min_signers,
            max_signers,
            "node_config.json".to_string(),
        )
    }
}

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    Identifier::derive(&bytes).unwrap()
}

// Active signing session tracking
pub struct ActiveSigning {
    pub sign_id: u64,
    pub message: Vec<u8>,
    pub selected_peers: Vec<PeerId>,
    pub nonces: frost::round1::SigningNonces,
    pub commitments: BTreeMap<Identifier, frost::round1::SigningCommitments>,
    pub signature_shares: BTreeMap<Identifier, frost::round2::SignatureShare>,
    pub signing_package: Option<frost::SigningPackage>,
    pub is_coordinator: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_deserialization() {
        let json_str = r#"{
            "allowed_peers": [
                {
                    "public_key": "12D3KooWRdtE2nFybk8eMyp3D9B4NvunUYqpN6JDvBcVPTcrDsbF",
                    "name": "node-four"
                }
            ],
            "key_data": {
                "public_key_b58": "12D3KooWQDHzW448RmDoUz1KbMfuD4XqeojRJDsxqUZSEYo7FSUz",
                "encrypted_private_key_b64": "EnCF8bEe3tVyMV0EUIK29bOMNjH7gT7mx4ATyBr4WSdphw5ETfm1YdQHDAg+CzBBjt7K2FSbwv8Qkj1y3N4jTU/FkGHggfkwDDl5XkDc5rXi2BW/",
                "encryption_params": {
                    "kdf": "argon2id",
                    "salt_b64": "TnErEFlx9F1BeU8mJcFzKQ",
                    "iv_b64": "hybTge0qoPaxNUhP"
                }
            }
        }"#;

        let config: Config = serde_json::from_str(json_str).expect("Failed to deserialize");
        assert_eq!(config.allowed_peers.len(), 1);
        assert_eq!(
            config.key_data.public_key_b58,
            "12D3KooWQDHzW448RmDoUz1KbMfuD4XqeojRJDsxqUZSEYo7FSUz"
        );
        assert!(config.dkg_keys.is_none());
    }
}
