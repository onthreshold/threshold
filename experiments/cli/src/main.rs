mod errors;
mod rpc_client;

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
use libp2p::identity::Keypair;
use rpc_client::{
    rpc_create_deposit_intent, rpc_send_direct_message, rpc_spend, rpc_start_signing,
};
use std::{fs, path::PathBuf};

use node::{start_node::start_node, Config, EncryptionParams, KeyData, PeerData};

use crate::errors::{CliError, KeygenError};

fn get_key_file_path() -> Result<PathBuf, KeygenError> {
    let proj_dirs = ProjectDirs::from("", "", "TheVault").ok_or_else(|| {
        KeygenError::DirectoryCreation("Failed to determine project directory".into())
    })?;

    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir).map_err(|e| KeygenError::DirectoryCreation(e.to_string()))?;

    Ok(config_dir.join("config.json"))
}

fn get_log_file_path() -> Result<PathBuf, KeygenError> {
    let proj_dirs = ProjectDirs::from("", "", "TheVault").ok_or_else(|| {
        KeygenError::DirectoryCreation("Failed to determine project directory".into())
    })?;

    let log_dir = proj_dirs.config_dir();
    let path = log_dir.join("node.log");
    Ok(path)
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
    let password = rpassword::prompt_password("Enter password: ").map_err(KeygenError::Io)?;

    let confirm = rpassword::prompt_password("Confirm password: ").map_err(KeygenError::Io)?;

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
    Setup {
        #[arg(short, long)]
        output: Option<String>,
        #[arg(short, long)]
        allowed_peers: Option<Vec<String>>,
    },
    /// Run the node and connect to the network
    Run {
        #[arg(short, long)]
        config: Option<String>,
        #[arg(short, long)]
        grpc_port: Option<u16>,
        #[arg(short, long)]
        log_file: Option<String>,
        #[arg(short = 'n', long)]
        max_signers: Option<u16>,
        #[arg(short = 't', long)]
        min_signers: Option<u16>,
    },
    Spend {
        amount: u64,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    StartSigning {
        hex_message: String,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    SendDirectMessage {
        peer_id: String,
        message: String,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    Deposit {
        peer_id: String,
        amount: u64,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
}

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup {
            output,
            allowed_peers,
        } => {
            setup_config(output, allowed_peers).map_err(|e| {
                println!("Keygen Error: {}", e);
                CliError::KeygenError(e)
            })?;
        }
        Commands::Run {
            config,
            grpc_port,
            log_file,
            max_signers,
            min_signers,
        } => {
            start_node_cli(config, grpc_port, log_file, max_signers, min_signers)
                .await
                .map_err(|_| CliError::NodeError)?;
        }
        Commands::Spend { amount, endpoint } => {
            rpc_spend(endpoint, amount)
                .await
                .map_err(CliError::RpcError)?;
        }
        Commands::StartSigning {
            hex_message,
            endpoint,
        } => {
            rpc_start_signing(endpoint, hex_message)
                .await
                .map_err(CliError::RpcError)?;
        }
        Commands::SendDirectMessage {
            peer_id,
            message,
            endpoint,
        } => {
            rpc_send_direct_message(endpoint, peer_id, message)
                .await
                .map_err(CliError::RpcError)?;
        }
        Commands::Deposit {
            peer_id,
            amount,
            endpoint,
        } => {
            rpc_create_deposit_intent(endpoint, peer_id, amount)
                .await
                .map_err(CliError::RpcError)?;
        }
    };

    Ok(())
}

fn setup_config(
    output: Option<String>,
    allowed_peers: Option<Vec<String>>,
) -> Result<(), KeygenError> {
    let keypair = Keypair::generate_ed25519();
    let public_key_b58 = keypair.public().to_peer_id().to_base58();

    let user_password = get_password()?;

    let (encrypted_private_key, encryption_params) = encrypt_private_key(&keypair, &user_password)?;

    let key_data = KeyData {
        public_key_b58: public_key_b58.clone(),
        encrypted_private_key_b64: encrypted_private_key,
        encryption_params,
    };

    let allowed_peer_ids = allowed_peers.unwrap_or_default();

    let allowed_peer_data = allowed_peer_ids
        .iter()
        .map(|peer_id| PeerData {
            public_key: peer_id.to_string(),
            name: peer_id.to_string(),
        })
        .collect();

    let config = Config {
        allowed_peers: allowed_peer_data,
        key_data,
        dkg_keys: None,
        log_file_path: Some(get_log_file_path()?),
    };

    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| KeygenError::Io(std::io::Error::other(e)))?;

    let key_file_path = if let Some(output) = output {
        let path = PathBuf::from(output);
        if path.is_dir() {
            return Err(KeygenError::KeyFileNotFound(format!(
                "The path {} is a directory",
                path.display()
            )));
        } else {
            path
        }
    } else {
        get_key_file_path()?
    };

    fs::write(&key_file_path, json).map_err(KeygenError::Io)?;

    println!(
        "Key data has been saved to {} with the peer id {}. To modify the allowed peers, edit the config file.",
        key_file_path.display(),
        public_key_b58
    );

    Ok(())
}

async fn start_node_cli(
    config_filepath: Option<String>,
    grpc_port: Option<u16>,
    log_file: Option<String>,
    max_signers: Option<u16>,
    min_signers: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    start_node(
        max_signers,
        min_signers,
        config_filepath,
        grpc_port,
        log_file.map(PathBuf::from),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
