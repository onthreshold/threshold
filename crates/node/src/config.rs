use crate::{NodeError, PeerData, key_manager};
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
    pub save_keys: bool,
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
    pub save_keys: bool,
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
            save_keys: true,
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
            save_keys: self.save_keys,
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
            save_keys: config_store.save_keys,
        };

        Ok(node_config)
    }

    pub fn save_dkg_keys(
        &mut self,
        private_key_package: &frost::keys::KeyPackage,
        pubkey_package: &frost::keys::PublicKeyPackage,
    ) -> Result<(), NodeError> {
        let password = match std::env::var("KEY_PASSWORD") {
            Ok(pw) => pw,
            Err(_) => crate::utils::key_manager::get_password_from_prompt()?,
        };

        let private_key_bytes = private_key_package.serialize().map_err(|e| {
            NodeError::Error(format!("Failed to serialize private key package: {e}"))
        })?;

        let (encrypted_private_key_b64, iv_b64) = crate::utils::key_manager::encrypt_private_key(
            &private_key_bytes,
            &password,
            &self.key_data.encryption_params.salt_b64,
        )?;

        let pubkey_bytes = pubkey_package.serialize().map_err(|e| {
            NodeError::Error(format!("Failed to serialize public key package: {e}"))
        })?;
        let pubkey_package_b64 = BASE64.encode(pubkey_bytes);

        let dkg_keys = DkgKeys {
            encrypted_private_key_package_b64: encrypted_private_key_b64,
            dkg_encryption_params: EncryptionParams {
                kdf: "argon2id".to_string(),
                salt_b64: self.key_data.encryption_params.salt_b64.clone(),
                iv_b64,
            },
            pubkey_package_b64,
        };

        self.dkg_keys = Some(dkg_keys);
        if self.save_keys {
            self.save_to_keys_file()?;
        }

        Ok(())
    }

    pub fn load_dkg_keys(
        &self,
    ) -> Result<Option<(frost::keys::KeyPackage, frost::keys::PublicKeyPackage)>, NodeError> {
        if let Some(dkg_keys) = &self.dkg_keys {
            let password = match std::env::var("KEY_PASSWORD") {
                Ok(pw) => pw,
                Err(_) => key_manager::get_password_from_prompt()?,
            };

            let private_key_bytes = key_manager::decrypt_private_key(
                &dkg_keys.encrypted_private_key_package_b64,
                &password,
                &dkg_keys.dkg_encryption_params,
            )
            .map_err(|e| NodeError::Error(format!("Failed to decrypt private key: {e}")))?;

            let private_key =
                frost::keys::KeyPackage::deserialize(&private_key_bytes).map_err(|e| {
                    NodeError::Error(format!("Failed to deserialize private key package: {e}"))
                })?;

            let pubkey_bytes = BASE64.decode(&dkg_keys.pubkey_package_b64).map_err(|e| {
                NodeError::Error(format!("Failed to decode public key package: {e}"))
            })?;
            let pubkey =
                frost::keys::PublicKeyPackage::deserialize(&pubkey_bytes).map_err(|e| {
                    NodeError::Error(format!("Failed to deserialize public key package: {e}"))
                })?;

            Ok(Some((private_key, pubkey)))
        } else {
            Ok(None)
        }
    }
}

pub struct NodeConfigBuilder {
    key_file_path: Option<PathBuf>,
    config_file_path: Option<PathBuf>,
    log_file_path: Option<PathBuf>,
    password: Option<String>,

    key_data: Option<KeyData>,
    dkg_keys: Option<DkgKeys>,
    allowed_peers: Option<Vec<PeerData>>,
    database_directory: Option<PathBuf>,
    grpc_port: Option<u16>,
    libp2p_udp_port: Option<u16>,
    libp2p_tcp_port: Option<u16>,
    confirmation_depth: Option<u32>,
    monitor_start_block: Option<u32>,
    min_signers: Option<u16>,
    max_signers: Option<u16>,
    save_keys: Option<bool>,
}

impl Default for NodeConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeConfigBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            key_file_path: None,
            config_file_path: None,
            log_file_path: None,
            password: None,

            key_data: None,
            dkg_keys: None,
            allowed_peers: None,
            database_directory: None,
            grpc_port: None,
            libp2p_udp_port: None,
            libp2p_tcp_port: None,
            confirmation_depth: None,
            monitor_start_block: None,
            min_signers: None,
            max_signers: None,
            save_keys: None,
        }
    }
    #[must_use]
    pub fn key_file_path(mut self, path: PathBuf) -> Self {
        self.key_file_path = Some(path);
        self
    }

    #[must_use]
    pub fn config_file_path(mut self, path: PathBuf) -> Self {
        self.config_file_path = Some(path);
        self
    }

    #[must_use]
    pub fn log_file_path(mut self, path: Option<PathBuf>) -> Self {
        self.log_file_path = path;
        self
    }

    #[must_use]
    pub fn password<S: Into<String>>(mut self, password: S) -> Self {
        self.password = Some(password.into());
        self
    }

    #[must_use]
    pub fn key_data(mut self, data: KeyData) -> Self {
        self.key_data = Some(data);
        self
    }

    #[must_use]
    pub fn dkg_keys(mut self, keys: DkgKeys) -> Self {
        self.dkg_keys = Some(keys);
        self
    }

    #[must_use]
    pub fn allowed_peers(mut self, peers: Vec<PeerData>) -> Self {
        self.allowed_peers = Some(peers);
        self
    }

    #[must_use]
    pub fn database_directory<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.database_directory = Some(path.into());
        self
    }

    #[must_use]
    pub const fn grpc_port(mut self, port: u16) -> Self {
        self.grpc_port = Some(port);
        self
    }

    #[must_use]
    pub const fn libp2p_udp_port(mut self, port: u16) -> Self {
        self.libp2p_udp_port = Some(port);
        self
    }

    #[must_use]
    pub const fn libp2p_tcp_port(mut self, port: u16) -> Self {
        self.libp2p_tcp_port = Some(port);
        self
    }

    #[must_use]
    pub const fn confirmation_depth(mut self, depth: u32) -> Self {
        self.confirmation_depth = Some(depth);
        self
    }

    #[must_use]
    pub const fn monitor_start_block(mut self, block: u32) -> Self {
        self.monitor_start_block = Some(block);
        self
    }

    #[must_use]
    pub const fn min_signers(mut self, min: u16) -> Self {
        self.min_signers = Some(min);
        self
    }

    #[must_use]
    pub const fn max_signers(mut self, max: u16) -> Self {
        self.max_signers = Some(max);
        self
    }

    #[must_use]
    pub const fn save_keys(mut self, save_keys: bool) -> Self {
        self.save_keys = Some(save_keys);
        self
    }

    pub fn build(self) -> Result<NodeConfig, NodeError> {
        let key_file_path = self.key_file_path.ok_or_else(|| {
            NodeError::Error("key_file_path must be provided when building NodeConfig".into())
        })?;

        let config_file_path = self.config_file_path.ok_or_else(|| {
            NodeError::Error("config_file_path must be provided when building NodeConfig".into())
        })?;

        let password = self.password.ok_or_else(|| {
            NodeError::Error("password must be provided when building NodeConfig".into())
        })?;

        let mut cfg = NodeConfig::new(
            key_file_path,
            config_file_path,
            self.log_file_path,
            &password,
        )
        .map_err(|e| NodeError::Error(format!("Failed to create NodeConfig: {e}")))?;

        if let Some(save_keys) = self.save_keys {
            cfg.save_keys = save_keys;
        }

        if let Some(data) = self.key_data {
            cfg.key_data = data;
        }
        if let Some(keys) = self.dkg_keys {
            cfg.dkg_keys = Some(keys);
        }
        if let Some(peers) = self.allowed_peers {
            cfg.allowed_peers = peers;
        }
        if let Some(db_dir) = self.database_directory {
            cfg.database_directory = db_dir;
        }
        if let Some(p) = self.grpc_port {
            cfg.grpc_port = p;
        }
        if let Some(p) = self.libp2p_udp_port {
            cfg.libp2p_udp_port = p;
        }
        if let Some(p) = self.libp2p_tcp_port {
            cfg.libp2p_tcp_port = p;
        }
        if let Some(d) = self.confirmation_depth {
            cfg.confirmation_depth = d;
        }
        if let Some(b) = self.monitor_start_block {
            cfg.monitor_start_block = b;
        }
        if let Some(mn) = self.min_signers {
            cfg.min_signers = Some(mn);
        }
        if let Some(mx) = self.max_signers {
            cfg.max_signers = Some(mx);
        }

        Ok(cfg)
    }
}
