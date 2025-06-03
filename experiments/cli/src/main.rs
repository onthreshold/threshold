mod errors;
mod key_manager;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use argon2::{
    password_hash::{
        rand_core::{OsRng, RngCore},
        SaltString,
    },
    Argon2,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use key_manager::{handle_key_error_and_exit, load_and_decrypt_keypair};
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use node::{swarm_manager::build_swarm, NodeState};

use crate::errors::{CliError, KeygenError};
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
    let proj_dirs = ProjectDirs::from("", "", "TheVault").ok_or_else(|| {
        KeygenError::DirectoryCreation("Failed to determine project directory".into())
    })?;

    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir).map_err(|e| KeygenError::DirectoryCreation(e.to_string()))?;

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

fn encrypt_private_key(
    keypair: &Keypair,
    password: &str,
) -> Result<(String, EncryptionParams), KeygenError> {
    let salt = SaltString::generate(&mut OsRng);
    let key = generate_key(password, &salt)?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let private_key_bytes = keypair
        .to_protobuf_encoding()
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
    let password =
        rpassword::prompt_password("Enter password: ").map_err(|e| KeygenError::Io(e))?;

    let confirm =
        rpassword::prompt_password("Confirm password: ").map_err(|e| KeygenError::Io(e))?;

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
    /// Generate a new keypair and save it to a file set by the --output flag
    GenerateKey {
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Run the node and connect to the network
    Run {
        #[arg(short, long)]
        file_path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateKey { output } => {
            generate_keypair(output).map_err(|e| {
                println!("Keygen Error: {}", e);
                CliError::KeygenError(e)
            })?;
        }
        Commands::Run { file_path } => {
            start_node(file_path)
                .await
                .map_err(|_| CliError::NodeError)?;
        }
    };

    Ok(())
}

fn generate_keypair(output: Option<String>) -> Result<(), KeygenError> {
    let keypair = Keypair::generate_ed25519();
    let public_key_b58 = keypair.public().to_peer_id().to_base58();

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

    fs::write(&key_file_path, json).map_err(|e| KeygenError::Io(e))?;

    println!(
        "Key data has been saved to {} with the peer id {}",
        key_file_path.display(),
        public_key_b58
    );

    Ok(())
}

async fn start_node(file_path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let keypair = match load_and_decrypt_keypair(file_path) {
        Ok(kp) => kp,
        Err(e) => handle_key_error_and_exit(e),
    };

    let max_signers = 5;
    let min_signers = 3;

    let mut swarm = build_swarm(keypair).map_err(|node_err: node::swarm_manager::NodeError| {
        let err_msg = format!("Failed to build swarm: {}", node_err.message);
        println!("{}", err_msg);
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, err_msg))
            as Box<dyn std::error::Error>
    })?;

    let mut node_state = NodeState::new(&mut swarm, min_signers, max_signers);
    let _ = node_state.main_loop().await;

    Ok(())
}

#[cfg(test)]
mod tests;
