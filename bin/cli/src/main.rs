mod errors;
mod rpc_client;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::{
    Argon2,
    password_hash::{
        SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use libp2p::identity::Keypair;
use rpc_client::{rpc_check_balance, rpc_create_deposit_intent, rpc_spend, rpc_start_signing};
use std::{fs, path::PathBuf};

use crate::{
    errors::{CliError, KeygenError},
    rpc_client::rpc_get_pending_deposit_intents,
};
use node::{
    NodeConfig, NodeConfigBuilder,
    config::{EncryptionParams, KeyData},
    start_node::start_node,
};

use types::errors::NodeError;

struct VaultConfigPath {
    key_file_path: PathBuf,
    config_file_path: PathBuf,
}

fn get_key_file_path() -> Result<VaultConfigPath, KeygenError> {
    let proj_dirs = ProjectDirs::from("", "", "TheVault").ok_or_else(|| {
        KeygenError::DirectoryCreation("Failed to determine project directory".into())
    })?;

    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir).map_err(|e| KeygenError::DirectoryCreation(e.to_string()))?;

    Ok(VaultConfigPath {
        key_file_path: config_dir.join("config.json"),
        config_file_path: config_dir.join("config.yaml"),
    })
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
    if let Ok(pw) = std::env::var("KEY_PASSWORD") {
        return Ok(pw);
    } // JUST FOR BOOTSTRAP.SH

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
        output_dir: Option<String>,
        #[arg(short, long)]
        file_name: Option<String>,
    },
    /// Run the node and connect to the network
    Run {
        #[arg(short = 'k', long)]
        key_file_path: Option<String>,
        #[arg(short = 'c', long)]
        config_file_path: Option<String>,
        #[arg(short = 'p', long)]
        grpc_port: Option<u16>,
        #[arg(short = 'u', long)]
        libp2p_udp_port: Option<u16>,
        #[arg(short = 't', long)]
        libp2p_tcp_port: Option<u16>,
        #[arg(short = 'd', long)]
        database_directory: Option<String>,
        #[arg(short = 'o', long)]
        min_signers: Option<u16>,
        #[arg(short = 'm', long)]
        max_signers: Option<u16>,
        #[arg(short = 'l', long)]
        log_file: Option<String>,
        #[arg(short = 'f', long)]
        confirmation_depth: Option<u32>,
        #[arg(short = 's', long)]
        monitor_start_block: Option<u32>,
        #[arg(long, action = clap::ArgAction::SetTrue)]
        use_mock_oracle: Option<bool>,
    },
    Spend {
        amount: u64,
        address_to: String,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    StartSigning {
        hex_message: String,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    Deposit {
        public_key: String,
        amount: u64,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    GetPendingDepositIntents {
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    CheckBalance {
        #[arg(short, long)]
        endpoint: Option<String>,
        address: String,
    },
}

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup {
            output_dir,
            file_name,
        } => {
            setup_config(output_dir, file_name).map_err(|e| {
                println!("Keygen Error: {e}");
                CliError::KeygenError(e)
            })?;
        }
        Commands::Run {
            key_file_path,
            config_file_path,
            grpc_port,
            libp2p_udp_port,
            libp2p_tcp_port,
            database_directory,
            log_file,
            min_signers,
            max_signers,
            confirmation_depth,
            monitor_start_block,
            use_mock_oracle,
        } => {
            start_node_cli(StartNodeConfigParams {
                key_file_path,
                config_file_path,
                grpc_port,
                libp2p_udp_port,
                libp2p_tcp_port,
                database_directory,
                log_file,
                min_signers,
                max_signers,
                confirmation_depth,
                monitor_start_block,
                use_mock_oracle,
            })
            .await
            .map_err(|e| CliError::NodeError(e.to_string()))?;
        }
        Commands::Spend {
            amount,
            endpoint,
            address_to,
        } => {
            rpc_spend(endpoint, amount, address_to)
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
        Commands::Deposit {
            amount,
            public_key,
            endpoint,
        } => {
            let response = rpc_create_deposit_intent(endpoint, amount, public_key)
                .await
                .map_err(CliError::RpcError)?;

            println!("Deposit intent created: {response:?}");
        }
        Commands::GetPendingDepositIntents { endpoint } => {
            rpc_get_pending_deposit_intents(endpoint)
                .await
                .map_err(CliError::RpcError)?;
        }
        Commands::CheckBalance { endpoint, address } => {
            rpc_check_balance(endpoint, address)
                .await
                .map_err(CliError::RpcError)?;
        }
    }

    Ok(())
}

fn setup_config(output_dir: Option<String>, file_name: Option<String>) -> Result<(), KeygenError> {
    let keypair = Keypair::generate_ed25519();
    let public_key_b58 = keypair.public().to_peer_id().to_base58();

    let user_password = get_password()?;

    let (encrypted_private_key, encryption_params) = encrypt_private_key(&keypair, &user_password)?;

    let key_data = KeyData {
        public_key_b58: public_key_b58.clone(),
        encrypted_private_key_b64: encrypted_private_key,
        encryption_params,
    };

    let paths = if let Some(output) = output_dir {
        let path = PathBuf::from(output);
        if path.is_dir() {
            VaultConfigPath {
                key_file_path: path.join(format!(
                    "{}.json",
                    file_name.clone().unwrap_or_else(|| "key".to_string())
                )),
                config_file_path: path.join(format!(
                    "{}.yaml",
                    file_name.unwrap_or_else(|| "config".to_string())
                )),
            }
        } else {
            return Err(KeygenError::KeyFileNotFound(
                "Output path is a file, not a directory. Please provide a directory path."
                    .to_string(),
            ));
        }
    } else {
        get_key_file_path()?
    };

    let config = NodeConfigBuilder::new()
        .key_file_path(paths.key_file_path.clone())
        .config_file_path(paths.config_file_path.clone())
        .log_file_path(get_log_file_path().ok())
        .password(&user_password)
        .key_data(key_data)
        .build()
        .map_err(|e| KeygenError::KeyFileNotFound(e.to_string()))?;

    config
        .save_to_keys_file()
        .map_err(|e| KeygenError::KeyFileNotFound(e.to_string()))?;

    config
        .save_to_file()
        .map_err(|e| KeygenError::KeyFileNotFound(e.to_string()))?;

    println!(
        "Key data has been saved to {} with the peer id {}. To modify the allowed peers and other configurations, edit the config file here: {}",
        paths.key_file_path.display(),
        public_key_b58,
        paths.config_file_path.display()
    );

    Ok(())
}

struct StartNodeConfigParams {
    key_file_path: Option<String>,
    config_file_path: Option<String>,
    grpc_port: Option<u16>,
    libp2p_udp_port: Option<u16>,
    libp2p_tcp_port: Option<u16>,
    database_directory: Option<String>,
    log_file: Option<String>,
    min_signers: Option<u16>,
    max_signers: Option<u16>,
    confirmation_depth: Option<u32>,
    monitor_start_block: Option<u32>,
    use_mock_oracle: Option<bool>,
}

async fn start_node_cli(params: StartNodeConfigParams) -> Result<(), NodeError> {
    let mut config = match NodeConfig::get_config(
        params.key_file_path.clone(),
        params.config_file_path.clone(),
    ) {
        Ok(config) => config,
        Err(e) => {
            return Err(NodeError::Error(format!("Failed to get config: {e}")));
        }
    };

    if let Some(port) = params.grpc_port {
        config.grpc_port = port;
    }

    if let Some(port) = params.libp2p_udp_port {
        config.libp2p_udp_port = port;
    }

    if let Some(port) = params.libp2p_tcp_port {
        config.libp2p_tcp_port = port;
    }

    if let Some(dir) = params.database_directory {
        config.database_directory = PathBuf::from(dir);
    }

    if let Some(min) = params.min_signers {
        config.min_signers = Some(min);
    }

    if let Some(max) = params.max_signers {
        config.max_signers = Some(max);
    }

    if let Some(depth) = params.confirmation_depth {
        config.confirmation_depth = depth;
    }

    if let Some(block) = params.monitor_start_block {
        config.monitor_start_block = block;
    }

    start_node(
        config,
        params.grpc_port,
        params.log_file.map(PathBuf::from),
        params.use_mock_oracle,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
