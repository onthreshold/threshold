use crate::{
    db::Db,
    dkg::DkgState,
    handler::Handler,
    key_manager::{encrypt_private_key, get_password_from_prompt},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use frost_secp256k1::{self as frost, Identifier};
use libp2p::PeerId;
use protocol::block::{ChainConfig, GenesisBlock, ValidatorInfo};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
};
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
    pub handlers: Vec<Box<dyn Handler<N>>>,
    pub db: D,

    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,

    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,

    // FROST signing
    pub active_signing: Option<ActiveSigning>,
    pub wallet: crate::wallet::SimpleWallet,
    pub pending_spends: std::collections::BTreeMap<u64, crate::wallet::PendingSpend>,

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
        let keys = DkgState::load_dkg_keys(config.clone())
            .map_err(|e| NodeError::Error(format!("Failed to load DKG keys: {}", e)))?;
        let dkg_state = DkgState::new(
            min_signers,
            max_signers,
            network_handle.peer_id(),
            keys.is_some(),
        )?;

        let mut node_state = NodeState {
            network_handle: network_handle.clone(),
            network_events_stream: network_events_emitter,
            peer_id: network_handle.peer_id(),
            min_signers,
            max_signers,
            db: storage_db,
            peers: HashSet::new(),
            rng: frost::rand_core::OsRng,
            active_signing: None,
            wallet: crate::wallet::SimpleWallet::new(),
            pending_spends: BTreeMap::new(),
            config,
            handlers: vec![Box::new(dkg_state)],
            pubkey_package: None,
            private_key_package: None,
        };

        if let Some((private_key, pubkey)) = keys {
            node_state.private_key_package = Some(private_key);
            node_state.pubkey_package = Some(pubkey);
        }

        Ok(node_state)
    }

    pub fn set_frost_keys(
        &mut self,
        private_key: Vec<u8>,
        public_key: Vec<u8>,
    ) -> Result<(), NodeError> {
        self.private_key_package =
            Some(frost::keys::KeyPackage::deserialize(&private_key).unwrap());
        self.pubkey_package =
            Some(frost::keys::PublicKeyPackage::deserialize(&public_key).unwrap());
        self.save_dkg_keys(
            &self.private_key_package.clone().unwrap(),
            &self.pubkey_package.clone().unwrap(),
        )?;

        Ok(())
    }

    pub fn save_dkg_keys(
        &mut self,
        private_key: &frost::keys::KeyPackage,
        pubkey: &frost::keys::PublicKeyPackage,
    ) -> Result<(), NodeError> {
        // Load existing config or create new one
        // Update DKG keys if they exist

        let password = get_password_from_prompt()
            .map_err(|e| NodeError::Error(format!("Failed to get password for DKG: {}", e)))?;

        // Serialize private key to bytes
        let private_key_bytes = private_key
            .serialize()
            .map_err(|e| NodeError::Error(format!("Failed to serialize private key: {}", e)))?;

        // Use existing salt from key_data, or generate a new one if empty
        let salt_b64 = if self.config.key_data.encryption_params.salt_b64.is_empty() {
            // Generate a new salt
            use frost::rand_core::RngCore;
            let mut salt = [0u8; 16];
            frost::rand_core::OsRng.fill_bytes(&mut salt);
            BASE64.encode(salt)
        } else {
            self.config.key_data.encryption_params.salt_b64.clone()
        };

        // Encrypt the private key package
        let (encrypted_private_key_b64, iv_b64) =
            encrypt_private_key(&private_key_bytes, &password, &salt_b64)
                .map_err(|e| NodeError::Error(format!("Failed to encrypt private key: {}", e)))?;

        // Serialize and base64 encode the public key package
        let pubkey_bytes = pubkey
            .serialize()
            .map_err(|e| NodeError::Error(format!("Failed to serialize public key: {}", e)))?;
        let pubkey_package_b64 = BASE64.encode(pubkey_bytes);

        self.config.set_dkg_keys(DkgKeys {
            encrypted_private_key_package_b64: encrypted_private_key_b64,
            dkg_encryption_params: EncryptionParams {
                kdf: "argon2id".to_string(),
                salt_b64,
                iv_b64,
            },
            pubkey_package_b64,
        });

        let validators = self
            .peers
            .iter()
            .map(|peer_id| ValidatorInfo {
                pub_key: peer_id.to_bytes(),
                stake: 100,
            })
            .collect();

        let chain_config = ChainConfig {
            block_time_seconds: 10,
            min_signers: self.min_signers,
            max_signers: self.max_signers,
            min_stake: 100,
            max_block_size: 1000,
        };

        let genesis_block = GenesisBlock::new(
            validators,
            chain_config,
            pubkey
                .serialize()
                .map_err(|e| NodeError::Error(format!("Failed to serialize public key: {}", e)))?,
        );

        self.db.insert_block(genesis_block.to_block())?;

        self.config.save_to_file()?;
        Ok(())
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
