use crate::{db::Db, dkg::DkgState, handler::Handler, signing::SigningState};
use frost_secp256k1::{self as frost, Identifier};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::PathBuf};
use swarm_manager::{Network, NetworkEvent};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::error;
use types::errors::NodeError;

pub mod db;
pub mod dkg;
pub mod grpc;
pub mod handler;
pub mod key_manager;
pub mod main_loop;
pub mod signing;
pub mod start_node;
pub mod swarm_manager;
pub mod wallet;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerData {
    pub name: String,
    pub public_key: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DkgKeys {
    pub encrypted_private_key_package_b64: String,
    pub dkg_encryption_params: EncryptionParams,
    pub pubkey_package_b64: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EncryptionParams {
    pub kdf: String,
    pub salt_b64: String,
    pub iv_b64: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KeyData {
    pub public_key_b58: String,
    pub encrypted_private_key_b64: String,
    pub encryption_params: EncryptionParams,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub allowed_peers: Vec<PeerData>,
    pub key_data: KeyData,
    pub dkg_keys: Option<DkgKeys>,
    pub log_file_path: Option<PathBuf>,
    #[serde(skip)]
    key_file_path: PathBuf,
    #[serde(skip)]
    config_file_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
pub struct KeyStore {
    key_data: KeyData,
    dkg_keys: Option<DkgKeys>,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigStore {
    allowed_peers: Vec<PeerData>,
    log_file_path: Option<PathBuf>,
    key_file_path: PathBuf,
}

impl NodeConfig {
    pub fn new(
        key_file_path: PathBuf,
        config_file_path: PathBuf,
        log_file_path: Option<PathBuf>,
    ) -> Self {
        NodeConfig {
            allowed_peers: Vec::new(),
            key_data: KeyData {
                public_key_b58: String::new(),
                encrypted_private_key_b64: String::new(),
                encryption_params: EncryptionParams {
                    kdf: String::new(),
                    salt_b64: String::new(),
                    iv_b64: String::new(),
                },
            },
            dkg_keys: None,
            log_file_path,
            key_file_path,
            config_file_path,
        }
    }

    pub fn save_to_file(&self) -> Result<(), NodeError> {
        let key_store = KeyStore {
            key_data: self.key_data.clone(),
            dkg_keys: self.dkg_keys.clone(),
        };

        let key_info_str = serde_json::to_string_pretty(&key_store)
            .map_err(|e| NodeError::Error(format!("Failed to serialize key data: {}", e)))?;

        fs::write(&self.key_file_path, key_info_str)
            .map_err(|e| NodeError::Error(format!("Failed to write key data: {}", e)))?;

        let config_store = ConfigStore {
            allowed_peers: self.allowed_peers.clone(),
            log_file_path: self.log_file_path.clone(),
            key_file_path: self.key_file_path.clone(),
        };

        let config_str: String = serde_yaml::to_string(&config_store).unwrap();

        fs::write(&self.config_file_path, config_str)
            .map_err(|e| NodeError::Error(format!("Failed to write config: {}", e)))?;

        Ok(())
    }

    pub fn set_dkg_keys(&mut self, dkg_keys: DkgKeys) {
        self.dkg_keys = Some(dkg_keys);
    }

    pub fn set_key_data(&mut self, key_data: KeyData) {
        self.key_data = key_data;
    }
}

pub struct NodeState<N: Network, D: Db> {
    pub handlers: Vec<Box<dyn Handler<N, D>>>,
    pub db: D,

    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,

    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,

    // FROST signing
    pub wallet: crate::wallet::SimpleWallet,

    pub config: NodeConfig,

    pub network_handle: N,

    pub network_events_stream: UnboundedReceiver<NetworkEvent>,
}

impl<N: Network, D: Db> NodeState<N, D> {
    pub fn new_from_config(
        network_handle: N,
        min_signers: u16,
        max_signers: u16,
        config: NodeConfig,
        storage_db: D,
        network_events_emitter: UnboundedReceiver<NetworkEvent>,
    ) -> Result<Self, NodeError> {
        let keys = key_manager::load_dkg_keys(config.clone())
            .map_err(|e| NodeError::Error(format!("Failed to load DKG keys: {}", e)))?;
        let dkg_state = DkgState::new()?;
        let signing_state = SigningState::new()?;

        let mut node_state = NodeState {
            network_handle: network_handle.clone(),
            network_events_stream: network_events_emitter,
            peer_id: network_handle.peer_id(),
            min_signers,
            max_signers,
            db: storage_db,
            peers: HashSet::new(),
            rng: frost::rand_core::OsRng,
            wallet: crate::wallet::SimpleWallet::new(),
            config,
            handlers: vec![Box::new(dkg_state), Box::new(signing_state)],
            pubkey_package: None,
            private_key_package: None,
        };

        if let Some((private_key, pubkey)) = keys {
            node_state.private_key_package = Some(private_key);
            node_state.pubkey_package = Some(pubkey);
        }

        Ok(node_state)
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
