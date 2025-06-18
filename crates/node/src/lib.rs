use crate::{
    handlers::{
        Handler, balance::BalanceState, consensus::ConsensusState, deposit::DepositIntentState,
        dkg::DkgState, signing::SigningState, withdrawl::SpendIntentState,
    },
    wallet::Wallet,
};
use abci::ChainInterface;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};
use argon2::{
    Argon2,
    password_hash::{
        SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use frost_secp256k1::{self as frost, Identifier};
use libp2p::{PeerId, identity::Keypair};
use oracle::oracle::Oracle;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::PathBuf};
use swarm_manager::Network;
use tokio::sync::broadcast;
use tracing::{error, info};
use types::{errors::NodeError, intents::DepositIntent, network_event::NetworkEvent};

pub mod grpc;
pub mod handlers;
pub mod main_loop;
pub mod start_node;

pub mod utils;
pub use utils::key_manager;
pub use utils::swarm_manager;
pub mod wallet;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub key_file_path: PathBuf,
    #[serde(skip)]
    pub config_file_path: PathBuf,
    pub database_directory: PathBuf,
    pub grpc_port: u16,
    pub libp2p_udp_port: u16,
    pub libp2p_tcp_port: u16,
    pub confirmation_depth: u32,
    pub monitor_start_block: u32,
    pub min_signers: Option<u16>,
    pub max_signers: Option<u16>,
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
    database_directory: PathBuf,
    grpc_port: u16,
    libp2p_udp_port: u16,
    libp2p_tcp_port: u16,
    confirmation_depth: u32,
    monitor_start_block: u32,
    min_signers: Option<u16>,
    max_signers: Option<u16>,
}

impl NodeConfig {
    pub fn new(
        key_file_path: PathBuf,
        config_file_path: PathBuf,
        log_file_path: Option<PathBuf>,
        password: &str,
    ) -> Result<Self, NodeError> {
        // Generate a new keypair
        let keypair = Keypair::generate_ed25519();
        let public_key_b58 = keypair.public().to_peer_id().to_base58();

        // Generate salt for encryption
        let salt = SaltString::generate(&mut OsRng);
        let salt_b64 = salt.to_string();

        // Derive encryption key from password
        let argon2 = Argon2::default();
        let mut key_bytes = vec![0u8; 32];
        argon2
            .hash_password_into(
                password.as_bytes(),
                salt.as_str().as_bytes(),
                &mut key_bytes,
            )
            .map_err(|e| NodeError::Error(format!("Argon2 key derivation failed: {e}")))?;

        // Generate random IV for AES encryption
        let mut iv = [0u8; 12];
        frost::rand_core::OsRng.fill_bytes(&mut iv);
        let nonce = Nonce::from_slice(&iv);

        // Get private key bytes
        let private_key_bytes = keypair
            .to_protobuf_encoding()
            .map_err(|e| NodeError::Error(format!("Failed to encode private key: {e}")))?;

        // Encrypt the private key
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
        let ciphertext = cipher
            .encrypt(nonce, private_key_bytes.as_ref())
            .map_err(|e| NodeError::Error(format!("AES encryption failed: {e}")))?;

        let encrypted_private_key_b64 = BASE64.encode(ciphertext);
        let iv_b64 = BASE64.encode(iv);

        let key_data = KeyData {
            public_key_b58,
            encrypted_private_key_b64,
            encryption_params: EncryptionParams {
                kdf: "argon2id".to_string(),
                salt_b64,
                iv_b64,
            },
        };

        Ok(Self {
            allowed_peers: Vec::new(),
            key_data,
            dkg_keys: None,
            log_file_path,
            key_file_path,
            config_file_path,
            database_directory: PathBuf::from("nodedb.db"),
            grpc_port: 50051,
            libp2p_udp_port: 0,
            libp2p_tcp_port: 0,
            confirmation_depth: 6,
            monitor_start_block: 0,
            min_signers: None,
            max_signers: None,
        })
    }

    pub fn save_to_keys_file(&self) -> Result<(), NodeError> {
        let key_store = KeyStore {
            key_data: self.key_data.clone(),
            dkg_keys: self.dkg_keys.clone(),
        };

        let key_info_str = serde_json::to_string_pretty(&key_store)
            .map_err(|e| NodeError::Error(format!("Failed to serialize key data: {e}")))?;

        fs::write(&self.key_file_path, key_info_str)
            .map_err(|e| NodeError::Error(format!("Failed to write key data: {e}")))?;

        Ok(())
    }

    pub fn save_to_file(&self) -> Result<(), NodeError> {
        let key_store = KeyStore {
            key_data: self.key_data.clone(),
            dkg_keys: self.dkg_keys.clone(),
        };

        let key_info_str = serde_json::to_string_pretty(&key_store)
            .map_err(|e| NodeError::Error(format!("Failed to serialize key data: {e}")))?;

        fs::write(&self.key_file_path, key_info_str)
            .map_err(|e| NodeError::Error(format!("Failed to write key data: {e}")))?;

        let config_store = ConfigStore {
            allowed_peers: self.allowed_peers.clone(),
            log_file_path: self.log_file_path.clone(),
            key_file_path: self.key_file_path.clone(),
            database_directory: self.database_directory.clone(),
            grpc_port: self.grpc_port,
            libp2p_udp_port: self.libp2p_udp_port,
            libp2p_tcp_port: self.libp2p_tcp_port,
            confirmation_depth: self.confirmation_depth,
            monitor_start_block: self.monitor_start_block,
            min_signers: self.min_signers,
            max_signers: self.max_signers,
        };

        let config_str: String = serde_yaml::to_string(&config_store).unwrap();

        fs::write(&self.config_file_path, config_str)
            .map_err(|e| NodeError::Error(format!("Failed to write config: {e}")))?;

        Ok(())
    }

    pub fn set_dkg_keys(&mut self, dkg_keys: DkgKeys) {
        self.dkg_keys = Some(dkg_keys);
    }

    pub fn set_key_data(&mut self, key_data: KeyData) {
        self.key_data = key_data;
    }

    pub const fn set_grpc_port(&mut self, port: u16) {
        self.grpc_port = port;
    }

    pub const fn set_libp2p_udp_port(&mut self, port: u16) {
        self.libp2p_udp_port = port;
    }

    pub const fn set_libp2p_tcp_port(&mut self, port: u16) {
        self.libp2p_tcp_port = port;
    }

    pub fn set_database_directory(&mut self, dir: PathBuf) {
        self.database_directory = dir;
    }

    pub const fn set_confirmation_depth(&mut self, depth: u32) {
        self.confirmation_depth = depth;
    }

    pub const fn set_monitor_start_block(&mut self, block: u32) {
        self.monitor_start_block = block;
    }

    pub const fn set_min_signers(&mut self, min: u16) {
        self.min_signers = Some(min);
    }

    pub const fn set_max_signers(&mut self, max: u16) {
        self.max_signers = Some(max);
    }
}

pub struct NodeState<N: Network, W: Wallet> {
    pub handlers: Vec<Box<dyn Handler<N, W>>>,

    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,

    pub rng: frost::rand_core::OsRng,
    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,

    // FROST signing
    pub wallet: W,
    pub config: NodeConfig,
    pub network_handle: N,
    pub network_events_stream: broadcast::Receiver<NetworkEvent>,

    pub oracle: Box<dyn Oracle>,
    pub chain_interface: Box<dyn ChainInterface>,
}

impl<N: Network, W: Wallet> NodeState<N, W> {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new_from_config(
        network_handle: &N,
        config: NodeConfig,
        network_events_sender: &broadcast::Sender<NetworkEvent>,
        deposit_intent_tx: broadcast::Sender<DepositIntent>,
        oracle: Box<dyn Oracle>,
        wallet: W,
        chain_interface: Box<dyn ChainInterface>,
    ) -> Result<Self, NodeError> {
        let keys = key_manager::load_dkg_keys(config.clone())
            .map_err(|e| NodeError::Error(format!("Failed to load DKG keys: {e}")))?;
        let dkg_state = DkgState::new()?;
        let signing_state = SigningState::new()?;
        let mut consensus_state = ConsensusState::new();

        for peer in &config.allowed_peers {
            if let Ok(peer_id) = peer.public_key.parse::<PeerId>() {
                consensus_state.validators.insert(peer_id);
            }
        }

        consensus_state.validators.insert(network_handle.peer_id());

        let mut deposit_intent_state = DepositIntentState::new(deposit_intent_tx);
        let withdrawl_intent_state = SpendIntentState::new();
        let balance_state = BalanceState::new();

        if let Ok(intents) = chain_interface.get_all_deposit_intents() {
            info!("Found {} deposit intents", intents.len());
            for intent in intents {
                if deposit_intent_state
                    .deposit_addresses
                    .insert(intent.deposit_address.clone())
                {
                    if let Err(e) = deposit_intent_state.deposit_intent_tx.send(intent.clone()) {
                        error!("Failed to notify deposit monitor of new address: {}", e);
                    }
                }
            }
        }

        let mut node_state = Self {
            network_handle: network_handle.clone(),
            network_events_stream: network_events_sender.subscribe(),
            peer_id: network_handle.peer_id(),
            min_signers,
            max_signers,
            peers: HashSet::new(),
            rng: frost::rand_core::OsRng,
            wallet,
            config,
            handlers: vec![
                Box::new(dkg_state),
                Box::new(signing_state),
                Box::new(consensus_state),
                Box::new(deposit_intent_state),
                Box::new(withdrawl_intent_state),
                Box::new(balance_state),
            ],
            pubkey_package: None,
            private_key_package: None,
            oracle,
            chain_interface,
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
