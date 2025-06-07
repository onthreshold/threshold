use std::collections::{BTreeMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use frost_secp256k1::{self as frost};
use libp2p::{PeerId, gossipsub};

use crate::{
    DkgKeys, EncryptionParams, NodeConfig,
    dkg::DkgState,
    key_manager::{decrypt_private_key, encrypt_private_key, get_password_from_prompt},
    swarm_manager::{Network, PrivateRequest},
};
use protocol::block::{ChainConfig, GenesisBlock, ValidatorInfo};
use types::errors::NodeError;

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
        if let (Some(private_key), Some(pubkey)) = (&self.private_key_package, &self.pubkey_package)
        {
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
                encrypt_private_key(&private_key_bytes, &password, &salt_b64)?;

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
            let password = get_password_from_prompt()?;

            // Decrypt the private key package
            let private_key_bytes = decrypt_private_key(
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
