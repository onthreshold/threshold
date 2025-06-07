use std::collections::{BTreeMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use frost_secp256k1::{self as frost};
use libp2p::{PeerId, gossipsub};

use crate::{
    NodeConfig,
    dkg::DkgState,
    key_manager::{decrypt_private_key, get_password_from_prompt},
};
use types::errors::NodeError;

impl DkgState {
    pub fn new(
        min_signers: u16,
        max_signers: u16,
        peer_id: PeerId,
        dkg_completed: bool,
    ) -> Result<Self, NodeError> {
        Ok(DkgState {
            min_signers,
            max_signers,
            rng: frost::rand_core::OsRng,
            peer_id,
            dkg_listeners: HashSet::new(),
            start_dkg_topic: gossipsub::IdentTopic::new("start-dkg"),
            round1_topic: gossipsub::IdentTopic::new("round1_topic"),
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            r1_secret_package: None,
            r2_secret_package: None,
            dkg_started: false,
            dkg_completed,
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
}
