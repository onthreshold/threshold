use types::errors::NodeError;

use crate::{
    NodeConfig, NodeState, db::RocksDb, grpc::grpc_handler::NodeControlService,
    key_manager::load_and_decrypt_keypair, swarm_manager::build_swarm,
};
use clients::{EsploraApiClient, WindowedConfirmedTransactionProvider};
use esplora_client::Builder;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tonic::transport::Server;
use tracing::{error, info};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub async fn start_node(
    max_signers: Option<u16>,
    min_signers: Option<u16>,
    config: NodeConfig,
    grpc_port: Option<u16>,
    log_file: Option<PathBuf>,
) -> Result<(), NodeError> {
    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry().with(env_filter);

    if let Some(log_path) = config.log_file_path.clone().or(log_file) {
        // File logging
        let log_dir = Path::new(&log_path);

        if !log_dir.exists() {
            info!("Creating log directory: {:?}", log_dir);
            if let Err(e) = std::fs::create_dir_all(log_dir) {
                error!(
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
        info!(
            "Logging initialized with file output: {}",
            log_path.display()
        );
    } else {
        // Console-only logging
        let console_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(true)
            .with_target(false);

        registry.with(console_layer).init();
        info!("Logging initialized with console output only");
    }

    let keypair = match load_and_decrypt_keypair(&config) {
        Ok(kp) => kp,
        Err(e) => {
            error!("Failed to decrypt key: {}", e);
            return Err(e);
        }
    };

    let max_signers = max_signers.unwrap_or(5);
    let min_signers = min_signers.unwrap_or(3);

    let allowed_peers = config.allowed_peers.clone();

    let (network_handle, mut swarm) =
        build_swarm(keypair.clone(), allowed_peers.clone()).expect("Failed to build swarm");

    let (deposit_intent_tx, deposit_intent_rx) = broadcast::channel(100);

    let mut node_state = NodeState::new_from_config(
        network_handle,
        min_signers,
        max_signers,
        config,
        RocksDb::new("nodedb.db"),
        swarm.network_events.clone(),
        deposit_intent_tx,
    )
    .expect("Failed to create node");

    let network_handle = node_state.network_handle.clone();

    let swarm_handle = tokio::spawn(async move {
        swarm.start().await;
    });

    let grpc_handle = tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", grpc_port.unwrap_or(50051))
            .parse()
            .unwrap();

        let node_control_service = NodeControlService::new(network_handle);

        info!("gRPC server listening on {}", addr);

        Server::builder()
            .add_service(node_control_service.into_server())
            .serve(addr)
            .await
            .expect("gRPC server failed");
    });

    let main_loop_handle = tokio::spawn(async move { node_state.start().await });

    let deposit_monitor_handle = tokio::spawn(async move {
        let mut client = EsploraApiClient::new(
            Builder::new("https://blockstream.info/api")
                .build_async()
                .unwrap(),
            100,
            Some(deposit_intent_rx),
        );

        client.poll_new_transactions(vec![]).await;
    });

    // Wait for either task to complete (they should run indefinitely)
    tokio::select! {
        result = grpc_handle => {
            match result {
                Ok(_) => info!("gRPC server stopped"),
                Err(e) => error!("gRPC server error: {}", e),
            }
        }
        result = swarm_handle => {
            match result {
                Ok(_) => info!("Swarm stopped"),
                Err(e) => error!("Swarm error: {}", e),
            }
        }
        result = main_loop_handle => {
            match result {
                Ok(Ok(_)) => info!("Main loop stopped"),
                Ok(Err(e)) => error!("Main loop error: {}", e),
                Err(e) => error!("Main loop task error: {}", e),
            }
        }
        result = deposit_monitor_handle => {
            match result {
                Ok(_) => info!("Deposit monitor stopped"),
                Err(e) => error!("Deposit monitor error: {}", e),
            }
        }
    }

    Ok(())
}
