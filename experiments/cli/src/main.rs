use libp2p::identity::Keypair;
use argon2::{
    password_hash::{
        rand_core::{OsRng, RngCore}, SaltString,
    },
    Argon2,
};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Serialize, Deserialize};
use std::{fs, path::PathBuf};
use directories::ProjectDirs;
use clap::{Parser, Subcommand};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeygenError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Password mismatch")]
    PasswordMismatch,
    
    #[error("Failed to create directory: {0}")]
    DirectoryCreation(String),
    
    #[error("Failed to encode key: {0}")]
    KeyEncoding(String),
    
    #[error("Failed to encrypt key: {0}")]
    Encryption(String),
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

fn get_key_file_path() -> Result<PathBuf, KeygenError> {
    let proj_dirs = ProjectDirs::from("", "", "TheVault")
        .ok_or_else(|| KeygenError::DirectoryCreation("Failed to determine project directory".into()))?;
    
    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir)
        .map_err(|e| KeygenError::DirectoryCreation(e.to_string()))?;
    
    Ok(config_dir.join("identity.key"))
}

fn generate_key(password: &str, salt: &SaltString) -> Result<Vec<u8>, KeygenError> {
    let argon2 = Argon2::default();
    let password_bytes = password.as_bytes();
    let mut key = vec![0u8; 32];
        
    argon2
        .hash_password_into(password_bytes, salt.as_str().as_bytes(), &mut key)
        .map_err(|e| KeygenError::Encryption(e.to_string()))?;
    Ok(key)
}

fn encrypt_private_key(keypair: &Keypair, password: &str) -> Result<(String, EncryptionParams), KeygenError> {
    let salt = SaltString::generate(&mut OsRng);
    let key = generate_key(password, &salt)?;
    
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let private_key_bytes = keypair.to_protobuf_encoding()
        .map_err(|e| KeygenError::KeyEncoding(e.to_string()))?;
    
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    
    let ciphertext = cipher
        .encrypt(nonce, private_key_bytes.as_ref())
        .map_err(|e| KeygenError::Encryption(e.to_string()))?;

    let params = EncryptionParams {
        kdf: "argon2id".to_string(),
        salt_b64: salt.to_string(),
        iv_b64: BASE64.encode(nonce_bytes),
    };
    
    Ok((BASE64.encode(ciphertext), params))
}

fn get_password() -> Result<String, KeygenError> {
    let password = rpassword::prompt_password("Enter password: ")
        .map_err(|e| KeygenError::Io(e))?;
    
    let confirm = rpassword::prompt_password("Confirm password: ")
        .map_err(|e| KeygenError::Io(e))?;
    
    if password != confirm {
        return Err(KeygenError::PasswordMismatch);
    }
    
    Ok(password)
}

#[derive(Parser)]
#[command(name = "keygen")]
#[command(about = "Generate public and private key pairs for the Vault.")]
#[command(version = "0.0.1")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new keypair and save it to a file defined by the --output flag
    Generate {
        #[arg(short, long)]
        output: Option<String>,
    },
}

fn main() -> Result<(), KeygenError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate { output } => {
            generate_keypair(output)?;
        }
    }
    
    Ok(())
}

fn generate_keypair(output: Option<String>) -> Result<(), KeygenError> {
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public().encode_protobuf();
    let public_key_b58 = bs58::encode(public_key).into_string();

    let user_password = get_password()?;
    
    let (encrypted_private_key, encryption_params) = encrypt_private_key(&keypair, &user_password)?;

    let key_data = KeyData {
        public_key_b58: public_key_b58.clone(),
        encrypted_private_key_b64: encrypted_private_key,
        encryption_params,
    };

    let json = serde_json::to_string_pretty(&key_data)
        .map_err(|e| KeygenError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    
    let key_file_path = if let Some(output) = output {
        let path = PathBuf::from(output);
        if path.is_dir() {
            path.join("identity.key")
        } else {
            path
        }
    } else {
        get_key_file_path()?
    };

    fs::write(&key_file_path, json)?;

    println!("Key data has been saved to {} with the peer id {}", key_file_path.display(), public_key_b58);
    Ok(())
}

#[cfg(test)]
mod tests;