use crate::{
    NodeState,
    errors::NodeError,
    grpc::grpc_handler::NodeControlService,
    key_manager::{get_config, get_key_file_path, load_and_decrypt_keypair},
    swarm_manager::build_swarm,
};
use std::path::{Path, PathBuf};
use tonic::transport::Server;
use tracing::{error, info};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub async fn start_node(
    max_signers: Option<u16>,
    min_signers: Option<u16>,
    config_filepath: Option<String>,
    grpc_port: Option<u16>,
    log_file: Option<PathBuf>,
) -> Result<(), NodeError> {
    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry().with(env_filter);

    let config = match get_config(config_filepath.clone()) {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to get config: {}", e);
            return Err(e);
        }
    };

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

    let config_file_path = if let Some(path) = config_filepath.clone() {
        path
    } else {
        match get_key_file_path() {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => {
                error!("Failed to get config file path: {}", e);
                std::process::exit(1);
            }
        }
    };

    let keypair = match load_and_decrypt_keypair(&config) {
        Ok(kp) => kp,
        Err(e) => {
            error!("Failed to decrypt key: {}", e);
            return Err(e);
        }
    };

    let max_signers = max_signers.unwrap_or(5);
    let min_signers = min_signers.unwrap_or(3);

    let allowed_peers = config.allowed_peers;

    let (network_handle, mut swarm, network_events_stream) =
        build_swarm(keypair.clone(), allowed_peers.clone()).expect("Failed to build swarm");

    let mut node_state = NodeState::new_from_config(
        network_handle,
        allowed_peers,
        min_signers,
        max_signers,
        config_file_path,
        network_events_stream,
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
    }

    Ok(())
}
