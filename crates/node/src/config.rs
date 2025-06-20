use crate::{DkgKeys, EncryptionParams, KeyData, NodeError, PeerData};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};
use argon2::{
    Argon2,
    password_hash::{
        SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use directories::ProjectDirs;
use frost_secp256k1::{self as frost};
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tracing::debug;

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
    pub key_data: KeyData,
    pub dkg_keys: Option<DkgKeys>,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigStore {
    pub allowed_peers: Vec<PeerData>,
    pub log_file_path: Option<PathBuf>,
    pub key_file_path: PathBuf,
    pub database_directory: PathBuf,
    pub grpc_port: u16,
    pub libp2p_udp_port: u16,
    pub libp2p_tcp_port: u16,
    pub confirmation_depth: u32,
    pub monitor_start_block: u32,
    pub min_signers: Option<u16>,
    pub max_signers: Option<u16>,
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

    pub fn get_key_file_path() -> Result<PathBuf, NodeError> {
        let proj_dirs = ProjectDirs::from("", "", "TheVault")
            .ok_or_else(|| NodeError::Error("Failed to determine project directory".into()))?;

        let config_dir = proj_dirs.config_dir();
        fs::create_dir_all(config_dir)
            .map_err(|e| NodeError::Error(format!("Failed to create config directory: {e}")))?;

        let path = config_dir.join("config.json");
        debug!("Using key file path: {}", path.display());
        Ok(path)
    }

    pub fn get_config_file_path(file_path_option: Option<String>) -> Result<PathBuf, NodeError> {
        if let Some(file_path_str) = file_path_option {
            let mut path = PathBuf::from(file_path_str);
            if path.is_dir() {
                path.push("config.yaml");
            }
            println!("Using config file path: {}", path.display());
            Ok(path)
        } else {
            let proj_dirs = ProjectDirs::from("", "", "TheVault")
                .ok_or_else(|| NodeError::Error("Failed to determine project directory".into()))?;
            let config_dir = proj_dirs.config_dir();
            Ok(config_dir.join("config.yaml"))
        }
    }

    pub fn get_config(
        key_file_path: Option<String>,
        config_file_path: Option<String>,
    ) -> Result<Self, NodeError> {
        let key_file_path = if let Some(key_path) = key_file_path {
            PathBuf::from(key_path)
        } else {
            Self::get_key_file_path()?
        };

        let config_file_path = if let Some(config_path) = config_file_path {
            PathBuf::from(config_path)
        } else {
            Self::get_config_file_path(None)?
        };

        let key_contents = fs::read_to_string(&key_file_path)
            .map_err(|e| NodeError::Error(format!("Failed to read config file: {e}")))?;

        let key_store = serde_json::from_str::<KeyStore>(&key_contents)
            .map_err(|e| NodeError::Error(format!("Failed to deserialize key file: {e}")))?;

        let config_contents = fs::read_to_string(&config_file_path)
            .map_err(|e| NodeError::Error(format!("Failed to read config file: {e}")))?;

        let config_store = serde_yaml::from_str::<ConfigStore>(&config_contents)
            .map_err(|e| NodeError::Error(format!("Failed to deserialize config file: {e}")))?;

        let node_config = Self {
            key_data: key_store.key_data,
            dkg_keys: key_store.dkg_keys,
            allowed_peers: config_store.allowed_peers,
            log_file_path: config_store.log_file_path,
            key_file_path,
            config_file_path,
            database_directory: config_store.database_directory,
            grpc_port: config_store.grpc_port,
            libp2p_udp_port: config_store.libp2p_udp_port,
            libp2p_tcp_port: config_store.libp2p_tcp_port,
            confirmation_depth: config_store.confirmation_depth,
            monitor_start_block: config_store.monitor_start_block,
            min_signers: config_store.min_signers,
            max_signers: config_store.max_signers,
        };

        Ok(node_config)
    }
}
