use crate::errors::KeygenError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use argon2::{password_hash::SaltString, Argon2};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use directories::ProjectDirs;
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process};

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

pub fn get_key_file_path(file_path_option: Option<String>) -> Result<PathBuf, KeygenError> {
    if let Some(file_path_str) = file_path_option {
        let mut path = PathBuf::from(file_path_str);
        if path.is_dir() {
            path.push("identity.key");
        }
        println!("Using key file path: {}", path.display());
        Ok(path)
    } else {
        let proj_dirs = ProjectDirs::from("", "", "TheVault").ok_or_else(|| {
            KeygenError::DirectoryCreation("Failed to determine project directory".into())
        })?;
        let config_dir = proj_dirs.config_dir();
        Ok(config_dir.join("identity.key"))
    }
}

fn derive_key_from_password(password: &str, salt_str: &str) -> Result<Vec<u8>, KeygenError> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let salt = SaltString::from_b64(salt_str)
        .map_err(|e| KeygenError::KeyDecoding(format!("Salt decoding failed: {}", e)))?;

    let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
        .map_err(|e| KeygenError::Decryption(format!("Argon2 key derivation failed: {}", e)))?;
    Ok(key)
}

fn decrypt_private_key(
    encrypted_private_key_b64: &str,
    password: &str,
    params: &EncryptionParams,
) -> Result<Vec<u8>, KeygenError> {
    let key_bytes = derive_key_from_password(password, &params.salt_b64)?;

    let iv_bytes = BASE64
        .decode(&params.iv_b64)
        .map_err(|e| KeygenError::KeyDecoding(format!("IV decoding failed: {}", e)))?;
    let nonce = Nonce::from_slice(&iv_bytes);

    let ciphertext = BASE64
        .decode(encrypted_private_key_b64)
        .map_err(|e| KeygenError::KeyDecoding(format!("Ciphertext decoding failed: {}", e)))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));

    let decrypted_private_key = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| KeygenError::Decryption(format!("AES decryption failed: {}", e)))?;

    Ok(decrypted_private_key)
}

fn get_password_from_prompt() -> Result<String, KeygenError> {
    rpassword::prompt_password("Enter password to decrypt identity key: ")
        .map_err(|e| KeygenError::Io(e))
}

pub fn load_and_decrypt_keypair(file_base_path: Option<String>) -> Result<Keypair, KeygenError> {
    let key_file_path = get_key_file_path(file_base_path)?;

    if !key_file_path.exists() {
        return Err(KeygenError::KeyFileNotFound(
            key_file_path.display().to_string(),
        ));
    }

    let file_content = fs::read_to_string(&key_file_path).map_err(KeygenError::Io)?;
    let key_data: KeyData =
        serde_json::from_str(&file_content).map_err(|e| KeygenError::JsonError(e))?;

    let password = get_password_from_prompt()?;

    let private_key_protobuf = decrypt_private_key(
        &key_data.encrypted_private_key_b64,
        &password,
        &key_data.encryption_params,
    )?;

    Keypair::from_protobuf_encoding(&private_key_protobuf).map_err(|e| {
        KeygenError::IdentityError(format!(
            "Failed to reconstruct keypair from protobuf: {}",
            e
        ))
    })
}

pub fn handle_key_error_and_exit(err: KeygenError) -> ! {
    eprintln!("Identity key error: {}", err);
    match err {
        KeygenError::KeyFileNotFound(_) => {
            eprintln!(
                "Please ensure 'identity.key' exists in the default configuration directory."
            );
            eprintln!("You can generate one using `vault generate-key --file-path <path>`).");
        }
        KeygenError::Decryption(_) | KeygenError::PasswordMismatch => {
            eprintln!("Failed to decrypt key. Check your password or the key file integrity.");
        }
        _ => {}
    }
    process::exit(1);
}
