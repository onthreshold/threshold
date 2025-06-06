use std::collections::{BTreeMap, HashSet};

use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};
use argon2::{Argon2, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use frost_secp256k1::{self as frost};
use libp2p::{PeerId, gossipsub};

use crate::{
    DkgKeys, EncryptionParams, NodeConfig,
    dkg::DkgState,
    swarm_manager::{Network, PrivateRequest},
};
use protocol::block::{ChainConfig, GenesisBlock, ValidatorInfo};
use types::errors::NodeError;

fn derive_key_from_password(
    password: &str,
    salt_str: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let salt =
        SaltString::from_b64(salt_str).map_err(|e| format!("Salt decoding failed: {}", e))?;

    let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
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
    let ciphertext = cipher
        .encrypt(nonce, private_key_data)
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
    let decrypted_private_key = cipher
        .decrypt(nonce, ciphertext.as_ref())
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

impl DkgState {
    pub fn get_public_key(&self) -> Option<frost::keys::PublicKeyPackage> {
        self.pubkey_package.clone()
    }

    pub fn get_private_key(&self) -> Option<frost::keys::KeyPackage> {
        self.private_key_package.clone()
    }
}

impl DkgState {
    pub fn new(
        min_signers: u16,
        max_signers: u16,
        peer_id: PeerId,
        config: NodeConfig,
    ) -> Result<Self, NodeError> {
        let mut dkg_state = DkgState {
            min_signers,
            max_signers,
            rng: frost::rand_core::OsRng,
            peer_id,
            dkg_listeners: HashSet::new(),
            config: config.clone(),
            start_dkg_topic: gossipsub::IdentTopic::new("start-dkg"),
            round1_topic: gossipsub::IdentTopic::new("round1_topic"),
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            r1_secret_package: None,
            r2_secret_package: None,
            pubkey_package: None,
            private_key_package: None,
            peers: HashSet::new(),
            dkg_started: false,
        };

        let keys = DkgState::load_dkg_keys(config)
            .map_err(|e| NodeError::Error(format!("Failed to load DKG keys: {}", e)))?;

        if let Some((private_key, pubkey)) = keys {
            dkg_state.private_key_package = Some(private_key);
            dkg_state.pubkey_package = Some(pubkey);
        }

        Ok(dkg_state)
    }

    pub fn save_dkg_keys(&mut self, network_handle: &impl Network) -> Result<(), NodeError> {
        // Load existing config or create new one
        // Update DKG keys if they exist
        if let (Some(private_key), Some(pubkey)) = (&self.private_key_package, &self.pubkey_package)
        {
            let password = get_password_for_dkg()
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
                encrypt_dkg_private_key(&private_key_bytes, &password, &salt_b64).map_err(|e| {
                    NodeError::Error(format!("Failed to encrypt private key: {}", e))
                })?;

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
                pubkey.serialize().map_err(|e| {
                    NodeError::Error(format!("Failed to serialize public key: {}", e))
                })?,
            );
            network_handle
                .send_self_request(
                    PrivateRequest::InsertBlock {
                        block: genesis_block.to_block(),
                    },
                    false,
                )
                .map_err(|e| NodeError::Error(format!("Failed to send genesis block: {:?}", e)))?;
        }

        let _ = self.config.save_to_file();
        Ok(())
    }

    pub fn load_dkg_keys(
        config: NodeConfig,
    ) -> Result<
        Option<(frost::keys::KeyPackage, frost::keys::PublicKeyPackage)>,
        Box<dyn std::error::Error>,
    > {
        if let Some(dkg_keys) = config.dkg_keys {
            let password = get_password_for_dkg()?;

            // Decrypt the private key package
            let private_key_bytes = decrypt_dkg_private_key(
                &dkg_keys.encrypted_private_key_package_b64,
                &password,
                &dkg_keys.dkg_encryption_params,
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
}
