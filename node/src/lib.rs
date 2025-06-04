use frost_secp256k1::{
    self as frost, Identifier,
    keys::dkg::{round1, round2},
};
use libp2p::{PeerId, gossipsub, identity::Keypair};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use argon2::{password_hash::SaltString, Argon2};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use crate::swarm_manager::build_swarm;

pub mod dkg;
pub mod grpc_service;
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
    pub dkg_listeners: HashSet<PeerId>,
    pub start_dkg_topic: libp2p::gossipsub::IdentTopic,
    pub round1_topic: libp2p::gossipsub::IdentTopic,
    pub r1_secret_package: Option<round1::SecretPackage>,
    pub peer_id: PeerId,
    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,
    pub peers: Vec<PeerId>,
    pub swarm: libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
    pub keypair: Keypair,
    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub r2_secret_package: Option<round2::SecretPackage>,

    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,

    // FROST signing
    pub active_signing: Option<ActiveSigning>,
    pub wallet: crate::wallet::SimpleWallet,
    pub pending_spends: std::collections::BTreeMap<u64, crate::wallet::PendingSpend>,
    
    // Config management
    pub config_file: String,
}

fn derive_key_from_password(password: &str, salt_str: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let salt = SaltString::from_b64(salt_str)
        .map_err(|e| format!("Salt decoding failed: {}", e))?;

    let mut key = vec![0u8; 32];
    argon2.hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
        .map_err(|e| format!("Argon2 key derivation failed: {}", e))?;
    Ok(key)
}

fn encrypt_dkg_private_key(
    private_key_data: &[u8],
    password: &str,
    salt_b64: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let key_bytes = derive_key_from_password(password, salt_b64)?;
    
    // Generate random IV
    let mut iv = [0u8; 12];
    use frost::rand_core::RngCore;
    frost::rand_core::OsRng.fill_bytes(&mut iv);
    let nonce = Nonce::from_slice(&iv);
    
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let ciphertext = cipher.encrypt(nonce, private_key_data)
        .map_err(|e| format!("AES encryption failed: {}", e))?;
    
    let encrypted_b64 = BASE64.encode(ciphertext);
    let iv_b64 = BASE64.encode(iv);
    
    Ok((encrypted_b64, iv_b64))
}

fn decrypt_dkg_private_key(
    encrypted_private_key_b64: &str,
    password: &str,
    params: &EncryptionParams,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let key_bytes = derive_key_from_password(password, &params.salt_b64)?;

    let iv_bytes = BASE64.decode(&params.iv_b64)?;
    let nonce = Nonce::from_slice(&iv_bytes);

    let ciphertext = BASE64.decode(encrypted_private_key_b64)?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let decrypted_private_key = cipher.decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| format!("AES decryption failed: {}", e))?;

    Ok(decrypted_private_key)
}

fn get_password_for_dkg() -> Result<String, Box<dyn std::error::Error>> {
    match std::env::var("KEY_PASSWORD") {
        Ok(pw) => Ok(pw),
        Err(_) => {
            use std::io::{self, Write};
            print!("Enter password to encrypt/decrypt DKG keys: ");
            io::stdout().flush()?;
            let password = rpassword::read_password()?;
            Ok(password)
        }
    }
}

impl NodeState {
    pub fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .unwrap_or(&peer_id.to_string())
            .clone()
    }

    pub fn save_dkg_keys(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Load existing config or create new one
        let mut config = if Path::new(&self.config_file).exists() {
            let config_str = fs::read_to_string(&self.config_file)?;
            serde_json::from_str::<Config>(&config_str)?
        } else {
            // For new configs, we need to create a dummy key_data
            // This is not ideal but maintains compatibility with the structure
            Config {
                allowed_peers: self.peers_to_names.iter()
                    .map(|(peer_id, name)| PeerData {
                        name: name.clone(),
                        public_key: peer_id.to_string(),
                    })
                    .collect(),
                key_data: KeyData {
                    public_key_b58: self.peer_id.to_string(),
                    encrypted_private_key_b64: String::new(),
                    encryption_params: EncryptionParams {
                        kdf: String::new(),
                        salt_b64: String::new(),
                        iv_b64: String::new(),
                    },
                },
                dkg_keys: None,
            }
        };

        // Update DKG keys if they exist
        if let (Some(private_key), Some(pubkey)) = (&self.private_key_package, &self.pubkey_package) {
            let password = get_password_for_dkg()?;
            
            // Serialize private key to bytes
            let private_key_bytes = private_key.serialize()?;
            
            // Use existing salt from key_data, or generate a new one if empty
            let salt_b64 = if config.key_data.encryption_params.salt_b64.is_empty() {
                // Generate a new salt
                use frost::rand_core::RngCore;
                let mut salt = [0u8; 16];
                frost::rand_core::OsRng.fill_bytes(&mut salt);
                BASE64.encode(salt)
            } else {
                config.key_data.encryption_params.salt_b64.clone()
            };
            
            // Encrypt the private key package
            let (encrypted_private_key_b64, iv_b64) = encrypt_dkg_private_key(
                &private_key_bytes,
                &password,
                &salt_b64
            )?;
            
            // Serialize and base64 encode the public key package
            let pubkey_bytes = pubkey.serialize()?;
            let pubkey_package_b64 = BASE64.encode(pubkey_bytes);
            
            config.dkg_keys = Some(DkgKeys {
                encrypted_private_key_package_b64: encrypted_private_key_b64,
                dkg_encryption_params: EncryptionParams {
                    kdf: "argon2id".to_string(),
                    salt_b64,
                    iv_b64,
                },
                pubkey_package_b64,
            });
        }

        // Save config
        let config_str = serde_json::to_string_pretty(&config)?;
        fs::write(&self.config_file, config_str)?;
        
        Ok(())
    }

    pub fn load_dkg_keys(config_path: &str) -> Result<Option<(frost::keys::KeyPackage, frost::keys::PublicKeyPackage)>, Box<dyn std::error::Error>> {
        if !Path::new(config_path).exists() {
            return Ok(None);
        }

        let config_str = fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config_str)?;

        if let Some(dkg_keys) = config.dkg_keys {
            let password = get_password_for_dkg()?;
            
            // Decrypt the private key package
            let private_key_bytes = decrypt_dkg_private_key(
                &dkg_keys.encrypted_private_key_package_b64,
                &password,
                &dkg_keys.dkg_encryption_params
            )?;
            
            // Deserialize the private key from decrypted bytes
            let private_key = frost::keys::KeyPackage::deserialize(&private_key_bytes)?;
            
            // Deserialize the public key from base64
            let pubkey_bytes = BASE64.decode(&dkg_keys.pubkey_package_b64)?;
            let pubkey = frost::keys::PublicKeyPackage::deserialize(&pubkey_bytes)?;
            
            Ok(Some((private_key, pubkey)))
        } else {
            Ok(None)
        }
    }

    pub fn new_from_config(
        keypair: Keypair,
        peer_data: Vec<PeerData>,
        min_signers: u16,
        max_signers: u16,
        config_file: String,
    ) -> Self {
        // Node State
        let swarm = build_swarm(keypair.clone()).expect("Failed to build swarm");
        let peer_id = *swarm.local_peer_id();

        let allowed_peers: Vec<PeerId> = peer_data
            .iter()
            .map(|peer| peer.public_key.parse().unwrap())
            .collect();

        let peers_to_names: BTreeMap<PeerId, String> = peer_data
            .iter()
            .map(|peer| (peer.public_key.parse().unwrap(), peer.name.clone()))
            .collect();

        let mut node_state = NodeState {
            allowed_peers,
            peers_to_names,
            dkg_listeners: HashSet::new(),
            round1_topic: gossipsub::IdentTopic::new("round1_topic"),
            start_dkg_topic: gossipsub::IdentTopic::new("start-dkg"),
            r1_secret_package: None,
            r2_secret_package: None,
            keypair,
            peer_id,
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            swarm,
            min_signers,
            max_signers,
            peers: Vec::new(),
            rng: frost::rand_core::OsRng,
            pubkey_package: None,
            private_key_package: None,
            active_signing: None,
            wallet: crate::wallet::SimpleWallet::new(),
            pending_spends: BTreeMap::new(),
            config_file: config_file.clone(),
        };
        
        // Try to load existing DKG keys
        match Self::load_dkg_keys(&config_file) {
            Ok(Some((private_key, pubkey))) => {
                println!("Loaded existing DKG keys from config");
                node_state.private_key_package = Some(private_key);
                node_state.pubkey_package = Some(pubkey);
            }
            Ok(None) => {
                println!("No existing DKG keys found, will perform DKG when requested");
            }
            Err(e) => {
                eprintln!("Failed to load DKG keys: {}", e);
            }
        }
        
        node_state
    }
    
    // Keep the old new() for backwards compatibility
    pub fn new(
        keypair: Keypair,
        peer_data: Vec<PeerData>,
        min_signers: u16,
        max_signers: u16,
    ) -> Self {
        Self::new_from_config(keypair, peer_data, min_signers, max_signers, "node_config.json".to_string())
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
        assert_eq!(config.key_data.public_key_b58, "12D3KooWQDHzW448RmDoUz1KbMfuD4XqeojRJDsxqUZSEYo7FSUz");
        assert!(config.dkg_keys.is_none());
    }
}
