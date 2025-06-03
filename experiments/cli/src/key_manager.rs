use libp2p::identity::Keypair;
use argon2::{
    password_hash::{SaltString},
    Argon2,
};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Serialize, Deserialize};
use std::{fs, path::PathBuf, process};
use directories::ProjectDirs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeyManagementError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Password input error: {0}")]
    PasswordInput(std::io::Error),
    #[error("Failed to create directory: {0}")]
    DirectoryCreation(String),
    #[error("Failed to decode key: {0}")]
    KeyDecoding(String),
    #[error("Failed to decrypt key: {0}")]
    Decryption(String),
    #[error("Key file not found at {0}")]
    KeyFileNotFound(String),
    #[error("JSON deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Libp2p identity error: {0}")]
    IdentityError(String),
}

#[derive(Serialize, Deserialize)]
struct EncryptionParams {
    kdf: String,
    salt_b64: String,
    iv_b64: String,
}

#[derive(Serialize, Deserialize)]
struct KeyData {
    public_key_b58: String, 
    encrypted_private_key_b64: String,
    encryption_params: EncryptionParams,
}

pub fn get_key_file_path(file_path_option: Option<String>) -> Result<PathBuf, KeyManagementError> {
    if let Some(file_path_str) = file_path_option {
        let mut path = PathBuf::from(file_path_str);
        if path.is_dir() {
            path.push("identity.key");
        }
        println!("Using key file path: {}", path.display());
        Ok(path)
    } else {
        let proj_dirs = ProjectDirs::from("", "", "TheVault")
            .ok_or_else(|| KeyManagementError::DirectoryCreation("Failed to determine project directory".into()))?;
        let config_dir = proj_dirs.config_dir();
        Ok(config_dir.join("identity.key"))
    }
}

fn derive_key_from_password(password: &str, salt_str: &str) -> Result<Vec<u8>, KeyManagementError> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let salt = SaltString::from_b64(salt_str)
        .map_err(|e| KeyManagementError::KeyDecoding(format!("Salt decoding failed: {}",e)))?;
    
        let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
        .map_err(|e| KeyManagementError::Decryption(format!("Argon2 key derivation failed: {}", e)))?;
    Ok(key)
}

fn decrypt_private_key(
    encrypted_private_key_b64: &str,
    password: &str,
    params: &EncryptionParams,
) -> Result<Vec<u8>, KeyManagementError> {
    let key_bytes = derive_key_from_password(password, &params.salt_b64)?;

    let iv_bytes = BASE64.decode(&params.iv_b64)
        .map_err(|e| KeyManagementError::KeyDecoding(format!("IV decoding failed: {}", e)))?;
    let nonce = Nonce::from_slice(&iv_bytes);

    let ciphertext = BASE64.decode(encrypted_private_key_b64)
        .map_err(|e| KeyManagementError::KeyDecoding(format!("Ciphertext decoding failed: {}", e)))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    
    let decrypted_private_key = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| KeyManagementError::Decryption(format!("AES decryption failed: {}", e)))?;
    
    Ok(decrypted_private_key)
}

fn get_password_from_prompt() -> Result<String, KeyManagementError> {
    rpassword::prompt_password("Enter password to decrypt identity key: ")
        .map_err(|e| KeyManagementError::PasswordInput(e))
}

pub fn load_and_decrypt_keypair(file_base_path: Option<String>) -> Result<Keypair, KeyManagementError> {
    let key_file_path = get_key_file_path(file_base_path)?;

    if !key_file_path.exists() {
        return Err(KeyManagementError::KeyFileNotFound(key_file_path.display().to_string()));
    }

    let file_content = fs::read_to_string(&key_file_path)
        .map_err(KeyManagementError::Io)?;
    let key_data: KeyData = serde_json::from_str(&file_content)?;

    let password = get_password_from_prompt()?;

    let private_key_protobuf = decrypt_private_key(
        &key_data.encrypted_private_key_b64,
        &password,
        &key_data.encryption_params,
    )?;
    
    Keypair::from_protobuf_encoding(&private_key_protobuf)
        .map_err(|e| KeyManagementError::IdentityError(format!("Failed to reconstruct keypair from protobuf: {}", e)))
}

pub fn handle_key_error_and_exit(err: KeyManagementError) -> ! {
    eprintln!("Identity key error: {}", err);
    match err {
        KeyManagementError::KeyFileNotFound(_) => {
            eprintln!("Please ensure 'identity.key' exists in the default configuration directory.");
            eprintln!("You can generate one using `vault generate-key --file-path <path>`).");
        }
        KeyManagementError::Decryption(_) | KeyManagementError::PasswordInput(_)=> {
            eprintln!("Failed to decrypt key. Check your password or the key file integrity.");
        }
        _ => {}
    }
    process::exit(1);
}
