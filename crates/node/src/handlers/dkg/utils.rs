use std::collections::{BTreeMap, HashSet};

use abci::{ChainMessage, ChainResponse};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use frost_secp256k1::{self as frost};
use libp2p::gossipsub;
use protocol::block::{ChainConfig, ValidatorInfo};

use crate::{
    DkgKeys, EncryptionParams, NodeConfig, NodeState,
    handlers::dkg::DkgState,
    key_manager::{decrypt_private_key, encrypt_private_key, get_password_from_prompt},
    wallet::Wallet,
};
use types::{errors::NodeError, network::Network};

impl DkgState {
    pub fn new() -> Result<Self, NodeError> {
        Ok(Self {
            dkg_listeners: HashSet::new(),
            round1_listeners: HashSet::new(),
            start_dkg_topic: gossipsub::IdentTopic::new("start-dkg"),
            round1_topic: gossipsub::IdentTopic::new("round1_topic"),
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            r1_secret_package: None,
            r2_secret_package: None,
            dkg_started: false,
        })
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

    pub async fn save_dkg_keys<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        private_key: &frost::keys::KeyPackage,
        pubkey: &frost::keys::PublicKeyPackage,
    ) -> Result<(), NodeError> {
        node.private_key_package = Some(private_key.clone());
        node.pubkey_package = Some(pubkey.clone());

        let password = match std::env::var("KEY_PASSWORD") {
            Ok(pw) => pw,
            Err(_) => get_password_from_prompt()?,
        };

        // Serialize private key to bytes
        let private_key_bytes = private_key
            .serialize()
            .map_err(|e| NodeError::Error(format!("Failed to serialize private key: {e}")))?;

        // Use existing salt from key_data, or generate a new one if empty
        let salt_b64 = if node.config.key_data.encryption_params.salt_b64.is_empty() {
            // Generate a new salt
            use frost::rand_core::RngCore;
            let mut salt = [0u8; 16];
            frost::rand_core::OsRng.fill_bytes(&mut salt);
            BASE64.encode(salt)
        } else {
            node.config.key_data.encryption_params.salt_b64.clone()
        };

        // Encrypt the private key package
        let (encrypted_private_key_b64, iv_b64) =
            encrypt_private_key(&private_key_bytes, &password, &salt_b64)
                .map_err(|e| NodeError::Error(format!("Failed to encrypt private key: {e}")))?;

        // Serialize and base64 encode the public key package
        let pubkey_bytes = pubkey
            .serialize()
            .map_err(|e| NodeError::Error(format!("Failed to serialize public key: {e}")))?;
        let pubkey_package_b64 = BASE64.encode(pubkey_bytes);

        node.config.set_dkg_keys(DkgKeys {
            encrypted_private_key_package_b64: encrypted_private_key_b64,
            dkg_encryption_params: EncryptionParams {
                kdf: "argon2id".to_string(),
                salt_b64,
                iv_b64,
            },
            pubkey_package_b64,
        });

        let mut validators: Vec<ValidatorInfo> = node
            .peers
            .iter()
            .map(|peer_id| ValidatorInfo {
                pub_key: peer_id.to_bytes(),
                stake: 100,
            })
            .collect();

        validators.sort_by(|a, b| a.pub_key.cmp(&b.pub_key));

        let chain_config = ChainConfig {
            block_time_seconds: 10,
            min_signers: node
                .config
                .min_signers
                .ok_or_else(|| NodeError::Error("Min signers not set".to_string()))?,
            max_signers: node
                .config
                .max_signers
                .ok_or_else(|| NodeError::Error("Max signers not set".to_string()))?,
            min_stake: 100,
            max_block_size: 1000,
        };

        let ChainResponse::CreateGenesisBlock { error: None } = node
            .chain_interface_tx
            .send_message_with_response(ChainMessage::CreateGenesisBlock {
                validators,
                chain_config,
                pubkey: pubkey.clone(),
            })
            .await?
        else {
            return Err(NodeError::Error(
                "Failed to create genesis block".to_string(),
            ));
        };

        node.config.save_to_keys_file()?;
        Ok(())
    }
}
