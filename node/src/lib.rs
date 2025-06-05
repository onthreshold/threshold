use crate::{db::Db, dkg::DkgState, errors::NodeError};
use frost_secp256k1::{self as frost, Identifier};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use swarm_manager::{NetworkEvent, NetworkHandle};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::error;

pub mod db;
pub mod dkg;
pub mod grpc;
pub mod main_loop;
pub mod protocol;
pub mod signing;
pub mod start_node;
pub mod swarm_manager;
pub mod wallet;

pub mod errors;
pub mod key_manager;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
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
    pub log_file_path: Option<PathBuf>,
}

pub struct NodeState {
    pub allowed_peers: Vec<PeerId>,
    pub peers_to_names: BTreeMap<PeerId, String>,

    // DKG
    pub dkg_state: DkgState,
    pub db: Db,

    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,

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

    pub network_events_stream: UnboundedReceiver<NetworkEvent>,
}

impl NodeState {
    pub fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .unwrap_or(&peer_id.to_string())
            .clone()
    }

    pub fn new_from_config(
        network_handle: NetworkHandle,
        peer_data: Vec<PeerData>,
        min_signers: u16,
        max_signers: u16,
        config_file: String,
        network_events_emitter: UnboundedReceiver<NetworkEvent>,
    ) -> Result<Self, NodeError> {
        let allowed_peers: Vec<PeerId> = peer_data
            .iter()
            .filter_map(|peer| {
                peer.public_key
                    .parse()
                    .map_err(|e| NodeError::Error(format!("Failed to parse peer data: {}", e)))
                    .ok()
            })
            .collect::<Vec<PeerId>>();

        let peers_to_names: BTreeMap<PeerId, String> = peer_data
            .iter()
            .filter_map(|peer| {
                let peer_id = peer
                    .public_key
                    .parse()
                    .map_err(|e| NodeError::Error(format!("Failed to parse peer data: {}", e)))
                    .ok()?;
                Some((peer_id, peer.name.clone()))
            })
            .collect::<BTreeMap<PeerId, String>>();

        let dkg_state = DkgState::new(
            network_handle.clone(),
            min_signers,
            max_signers,
            network_handle.peer_id(),
            peers_to_names.clone(),
            config_file.clone(),
        )?;

        Ok(NodeState {
            network_handle: network_handle.clone(),
            allowed_peers: allowed_peers.clone(),
            network_events_stream: network_events_emitter,
            peers_to_names: peers_to_names.clone(),
            peer_id: network_handle.peer_id(),
            min_signers,
            max_signers,
            db: Db::new("node_db.db"),
            dkg_state,
            peers: HashSet::new(),
            rng: frost::rand_core::OsRng,
            active_signing: None,
            wallet: crate::wallet::SimpleWallet::new(),
            pending_spends: BTreeMap::new(),
            config_file: config_file.clone(),
        })
    }
}

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    match Identifier::derive(&bytes) {
        Ok(identifier) => identifier,
        Err(e) => {
            error!("Failed to derive identifier: {}", e);
            panic!("Failed to derive identifier");
        }
    }
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
