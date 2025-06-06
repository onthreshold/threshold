use std::{fs, path::PathBuf};

use crate::{EncryptionParams, NodeConfig};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};
use argon2::{Argon2, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use directories::ProjectDirs;
use libp2p::identity::Keypair;
use tracing::debug;
use types::errors::NodeError;

pub fn get_key_file_path() -> Result<PathBuf, NodeError> {
    let proj_dirs = ProjectDirs::from("", "", "TheVault")
        .ok_or_else(|| NodeError::Error("Failed to determine project directory".into()))?;

    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir)
        .map_err(|e| NodeError::Error(format!("Failed to create config directory: {}", e)))?;

    let path = config_dir.join("config.json");
    debug!("Using key file path: {}", path.display());
    Ok(path)
}

pub fn get_config_file_path(file_path_option: Option<String>) -> Result<PathBuf, NodeError> {
    if let Some(file_path_str) = file_path_option {
        let mut path = PathBuf::from(file_path_str);
        if path.is_dir() {
            path.push("config.json");
        }
        println!("Using key file path: {}", path.display());
        Ok(path)
    } else {
        let proj_dirs = ProjectDirs::from("", "", "TheVault")
            .ok_or_else(|| NodeError::Error("Failed to determine project directory".into()))?;
        let config_dir = proj_dirs.config_dir();
        Ok(config_dir.join("config.json"))
    }
}

fn derive_key_from_password(password: &str, salt_str: &str) -> Result<Vec<u8>, NodeError> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let salt = SaltString::from_b64(salt_str)
        .map_err(|e| NodeError::Error(format!("Salt decoding failed: {}", e)))?;

    let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
        .map_err(|e| NodeError::Error(format!("Argon2 key derivation failed: {}", e)))?;
    Ok(key)
}

fn decrypt_private_key(
    encrypted_private_key_b64: &str,
    password: &str,
    params: &EncryptionParams,
) -> Result<Vec<u8>, NodeError> {
    let key_bytes = derive_key_from_password(password, &params.salt_b64)?;

    let iv_bytes = BASE64
        .decode(&params.iv_b64)
        .map_err(|e| NodeError::Error(format!("IV decoding failed: {}", e)))?;
    let nonce = Nonce::from_slice(&iv_bytes);

    let ciphertext = BASE64
        .decode(encrypted_private_key_b64)
        .map_err(|e| NodeError::Error(format!("Ciphertext decoding failed: {}", e)))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));

    let decrypted_private_key = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| NodeError::Error(format!("AES decryption failed: {}", e)))?;

    Ok(decrypted_private_key)
}

fn get_password_from_prompt() -> Result<String, NodeError> {
    rpassword::prompt_password("Enter password to decrypt identity key: ")
        .map_err(|e| NodeError::Error(e.to_string()))
}

pub fn get_config(config_filepath: Option<String>) -> Result<NodeConfig, NodeError> {
    let key_file_path = if let Some(config_path) = config_filepath {
        PathBuf::from(config_path)
    } else {
        get_key_file_path()?
    };

    debug!("Using key file path: {}", key_file_path.display());

    let config_contents = fs::read_to_string(&key_file_path)
        .map_err(|e| NodeError::Error(format!("Failed to read config file: {}", e)))?;

    debug!("Read config file");

    let config = serde_json::from_str::<NodeConfig>(&config_contents)
        .map_err(|e| NodeError::Error(format!("Failed to deserialize config file: {}", e)))?;

    debug!("Deserialized config file");

    Ok(config)
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

    Keypair::from_protobuf_encoding(&private_key_protobuf).map_err(|e| {
        NodeError::Error(format!(
            "Failed to reconstruct keypair from protobuf: {}",
            e
        ))
    })
}
