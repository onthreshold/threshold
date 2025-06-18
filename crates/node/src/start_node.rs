use oracle::{esplora::EsploraOracle, mock::MockOracle, oracle::Oracle};
use types::{errors::NodeError, intents::DepositIntent};

use crate::{
    NodeConfig, NodeState, db::RocksDb, grpc::grpc_handler::NodeControlService,
    key_manager::load_and_decrypt_keypair, swarm_manager::build_swarm, wallet::TaprootWallet,
};
use bitcoin::Network;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tonic::transport::Server;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub async fn start_node(
    config: NodeConfig,
    grpc_port: Option<u16>,
    log_file: Option<PathBuf>,
    use_mock_oracle: Option<bool>,
) -> Result<(), NodeError> {
    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let config_database_path = config.database_directory.clone();
    let config_grpc_port = config.grpc_port;
    let confirmation_depth = config.confirmation_depth;
    let monitor_start_block = config.monitor_start_block;

    let registry = tracing_subscriber::registry().with(env_filter);

    if let Some(log_path) = config.log_file_path.clone().or(log_file) {
        let log_dir = Path::new(&log_path);

        if !log_dir.exists() {
            tracing::info!("Creating log directory: {:?}", log_dir);
            if let Err(e) = std::fs::create_dir_all(log_dir) {
                tracing::error!(
                    "Failed to create log directory {}: {}",
                    log_dir.display(),
                    e
                );
                return Err(NodeError::Error(e.to_string()));
            }
        }

        let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, "node.log");

        let file_layer = fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true);

        let console_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(true)
            .with_target(false);

        registry.with(file_layer).with(console_layer).init();
        tracing::info!(
            "Logging initialized with file output: {}",
            log_path.display()
        );
    } else {
        let console_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(true)
            .with_target(false);

        registry.with(console_layer).init();
        tracing::info!("Logging initialized with console output only");
    }

    let keypair = match load_and_decrypt_keypair(&config) {
        Ok(kp) => kp,
        Err(e) => {
            tracing::error!("Failed to decrypt key: {}", e);
            return Err(e);
        }
    };

    let allowed_peers = config.allowed_peers.clone();

    let (network_handle, mut swarm) = build_swarm(
        keypair.clone(),
        config.libp2p_udp_port,
        config.libp2p_tcp_port,
        &allowed_peers,
    )
    .expect("Failed to build swarm");

    let (deposit_intent_tx, _) = broadcast::channel::<DepositIntent>(100);
    let is_testnet = dotenvy::var("IS_TESTNET")
        .unwrap_or_else(|_| "false".to_string())
        .parse()
        .unwrap();

    let mut oracle: Box<dyn Oracle> = if use_mock_oracle.unwrap_or(false) {
        Box::new(MockOracle::new(
            swarm.network_events.clone(),
            Some(deposit_intent_tx.clone()),
        ))
    } else {
        Box::new(EsploraOracle::new(
            if is_testnet {
                Network::Testnet
            } else {
                Network::Bitcoin
            },
            Some(100),
            Some(swarm.network_events.clone()),
            Some(deposit_intent_tx.clone()),
            confirmation_depth,
            monitor_start_block,
        ))
    };

    let mut node_state = NodeState::new_from_config(
        &network_handle,
        config,
        RocksDb::new(config_database_path.to_str().unwrap()),
        &swarm.network_events,
        deposit_intent_tx,
        oracle.clone(),
        TaprootWallet::new(oracle.clone(), Vec::new(), {
            let is_testnet: bool = std::env::var("IS_TESTNET")
                .unwrap_or_else(|_| String::from("false"))
                .parse()
                .unwrap_or(false);

            if is_testnet {
                bitcoin::Network::Testnet
            } else {
                bitcoin::Network::Bitcoin
            }
        }),
    )
    .expect("Failed to create node");

    let network_handle = node_state.network_handle.clone();

    let swarm_handle = tokio::spawn(async move {
        swarm.start().await;
    });

    let grpc_handle = tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", grpc_port.unwrap_or(config_grpc_port))
            .parse()
            .unwrap();

        let node_control_service = NodeControlService::new(network_handle);

        tracing::info!("gRPC server listening on {}", addr);

        Server::builder()
            .add_service(node_control_service.into_server())
            .serve(addr)
            .await
            .expect("gRPC server failed");
    });

    let main_loop_handle = tokio::spawn(async move { node_state.start().await });

    let deposit_monitor_handle = tokio::spawn(async move {
        oracle.poll_new_transactions(vec![]).await;
    });

    // Wait for either task to complete (they should run indefinitely)
    tokio::select! {
        result = grpc_handle => {
            match result {
                Ok(()) => tracing::info!("gRPC server stopped"),
                Err(e) => tracing::error!("gRPC server error: {}", e),
            }
        }
        result = swarm_handle => {
            match result {
                Ok(()) => tracing::info!("Swarm stopped"),
                Err(e) => tracing::error!("Swarm error: {}", e),
            }
        }
        _ = main_loop_handle => {
            tracing::info!("Main loop stopped");
        }
        result = deposit_monitor_handle => {
            match result {
                Ok(()) => tracing::info!("Deposit monitor stopped"),
                Err(e) => tracing::error!("Deposit monitor error: {}", e),
            }
        }
    }

    Ok(())
}
