use crate::{NodeConfig, config::EncryptionParams};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};
use argon2::{Argon2, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bip39::{Language, Mnemonic};
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::key::Secp256k1;
use bitcoin::{Address, CompressedPublicKey, Network, PrivateKey};
use frost::rand_core::RngCore;
use frost_secp256k1 as frost;
use libp2p::identity::Keypair;
use std::str::FromStr;
use types::errors::NodeError;

pub fn derive_key_from_password(password: &str, salt_str: &str) -> Result<Vec<u8>, NodeError> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let salt = SaltString::from_b64(salt_str)
        .map_err(|e| NodeError::Error(format!("Salt decoding failed: {e}")))?;

    let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
        .map_err(|e| NodeError::Error(format!("Argon2 key derivation failed: {e}")))?;
    Ok(key)
}

pub fn encrypt_private_key(
    private_key_data: &[u8],
    password: &str,
    salt_b64: &str,
) -> Result<(String, String), NodeError> {
    let key_bytes = derive_key_from_password(password, salt_b64)?;

    // Generate random IV
    let mut iv = [0u8; 12];
    frost::rand_core::OsRng.fill_bytes(&mut iv);
    let nonce = Nonce::from_slice(&iv);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let ciphertext = cipher
        .encrypt(nonce, private_key_data)
        .map_err(|e| NodeError::Error(format!("AES encryption failed: {e}")))?;

    let encrypted_b64 = BASE64.encode(ciphertext);
    let iv_b64 = BASE64.encode(iv);

    Ok((encrypted_b64, iv_b64))
}

pub fn decrypt_private_key(
    encrypted_private_key_b64: &str,
    password: &str,
    params: &EncryptionParams,
) -> Result<Vec<u8>, NodeError> {
    let key_bytes = derive_key_from_password(password, &params.salt_b64)?;

    let iv_bytes = BASE64
        .decode(&params.iv_b64)
        .map_err(|e| NodeError::Error(format!("IV decoding failed: {e}")))?;
    let nonce = Nonce::from_slice(&iv_bytes);

    let ciphertext = BASE64
        .decode(encrypted_private_key_b64)
        .map_err(|e| NodeError::Error(format!("Ciphertext decoding failed: {e}")))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));

    let decrypted_private_key = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| NodeError::Error(format!("AES decryption failed: {e}")))?;

    Ok(decrypted_private_key)
}

pub fn get_password_from_prompt() -> Result<String, NodeError> {
    rpassword::prompt_password("Enter password to decrypt identity key: ")
        .map_err(|e| NodeError::Error(e.to_string()))
}

pub fn load_and_decrypt_keypair(config_data: &NodeConfig) -> Result<Keypair, NodeError> {
    let password = match std::env::var("KEY_PASSWORD") {
        Ok(pw) => pw,
        Err(_) => get_password_from_prompt()?,
    };

    let private_key_protobuf = decrypt_private_key(
        &config_data.key_data.encrypted_private_key_b64,
        &password,
        &config_data.key_data.encryption_params,
    )?;

    Keypair::from_protobuf_encoding(&private_key_protobuf)
        .map_err(|e| NodeError::Error(format!("Failed to reconstruct keypair from protobuf: {e}")))
}

#[must_use]
pub fn generate_keys_from_mnemonic(mnemonic: &str) -> (Address, PrivateKey, CompressedPublicKey) {
    // Generate a new mnemonic (12 words)
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic).unwrap();

    // Convert to seed
    let seed = mnemonic.to_seed(""); // Empty passphrase

    // Create extended private key
    let secp = Secp256k1::new();
    let xprv = Xpriv::new_master(Network::Testnet, &seed).unwrap();

    // Derive key at standard path (m/84'/1'/0'/0/0 for signet P2WPKH)
    let derivation_path = DerivationPath::from_str("m/84'/1'/0'/0/0").unwrap();
    let derived_xprv = xprv.derive_priv(&secp, &derivation_path).unwrap();

    // Get the private key
    let private_key = PrivateKey::new(derived_xprv.private_key, Network::Testnet);
    let compressed_public_key: CompressedPublicKey =
        CompressedPublicKey::from_private_key(&secp, &private_key)
            .expect("Failed to convert public key to compressed public key");
    let address = Address::p2wpkh(&compressed_public_key, Network::Testnet);

    (address, private_key, compressed_public_key)
}
