use abci::{ChainInterfaceImpl, db::rocksdb::RocksDb, executor::TransactionExecutorImpl};
use consensus::{ConsensusInterface, ConsensusInterfaceImpl, ConsensusMessage};
use oracle::{esplora::EsploraOracle, mock::MockOracle, oracle::Oracle};
use types::network::network_protocol::Network;
use types::{errors::NodeError, intents::DepositIntent};

use crate::{
    NodeConfig, NodeState, key_manager::load_and_decrypt_keypair, swarm_manager::build_swarm,
    wallet::TaprootWallet,
};
use actix_web::{App, HttpResponse, HttpServer, web};
use bitcoin::Network as BitcoinNetwork;
use grpc::grpc_handler::NodeControlService;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::{signal, sync::broadcast};
use tonic::transport::Server;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

type PrometheusHandler = Arc<PrometheusHandle>;

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

    let prometheus_handle: Arc<PrometheusHandle> = {
        let builder = PrometheusBuilder::new();
        Arc::new(
            builder
                .install_recorder()
                .expect("failed to install Prometheus recorder"),
        )
    };

    let metrics_server_handle = tokio::spawn(async move {
        async fn metrics_endpoint(handler: web::Data<PrometheusHandler>) -> HttpResponse {
            metrics::counter!("metrics_scrape_requests_total").increment(1);
            HttpResponse::Ok()
                .content_type("text/plain")
                .body(handler.render())
        }

        async fn health_endpoint() -> HttpResponse {
            HttpResponse::Ok().content_type("text/plain").body("OK")
        }

        tracing::info!("Starting metrics server on 0.0.0.0:8080");

        let server = HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(prometheus_handle.clone()))
                .route("/metrics", web::get().to(metrics_endpoint))
                .route("/health", web::get().to(health_endpoint))
        })
        .bind(("0.0.0.0", 8080))
        .expect("Failed to bind metrics endpoint");

        tracing::info!("Metrics server bound successfully, starting to serve");

        server.run().await.expect("Metrics server failed");
    });

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

    let oracle: Box<dyn Oracle> = if use_mock_oracle.unwrap_or(false) {
        Box::new(MockOracle::new(
            swarm.network_events.clone(),
            Some(deposit_intent_tx.clone()),
        ))
    } else {
        Box::new(EsploraOracle::new(
            if is_testnet {
                BitcoinNetwork::Testnet
            } else {
                BitcoinNetwork::Bitcoin
            },
            Some(100),
            Some(swarm.network_events.clone()),
            Some(deposit_intent_tx.clone()),
            confirmation_depth,
            monitor_start_block,
        ))
    };

    let db = RocksDb::new(config_database_path.to_str().unwrap());

    let db_arc: Arc<RocksDb> = Arc::new(db.clone());

    let (mut chain_interface, chain_message_tx) = ChainInterfaceImpl::new(
        Box::new(db.clone()),
        Box::new(TransactionExecutorImpl::new(oracle.clone())),
    );

    let chain_interface_handle = tokio::spawn(async move {
        chain_interface.start().await;
    });

    let (mut consensus_interface, consensus_message_tx) = ConsensusInterfaceImpl::new();

    // Set up consensus interface with necessary components
    consensus_interface.set_chain_interface(chain_message_tx.clone());
    consensus_interface.set_peer_id(network_handle.peer_id());
    consensus_interface.set_network_events_tx(swarm.network_events.clone());

    // Set max validators (self + allowed peers)
    let max_validators = allowed_peers.len() + 1;
    consensus_interface.set_max_validators(max_validators);

    // Add validators from config
    for peer in &allowed_peers {
        if let Ok(peer_id) = peer.public_key.parse::<libp2p::PeerId>() {
            let _ = consensus_interface
                .handle_message(ConsensusMessage::AddValidator {
                    peer_id: peer_id.to_bytes(),
                })
                .await;
        }
    }

    // Add self as validator
    let _ = consensus_interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: network_handle.peer_id().to_bytes(),
        })
        .await;

    // Initialize consensus state from current chain state
    if let Err(e) = consensus_interface.initialize_from_chain_state().await {
        tracing::error!("Failed to initialize consensus from chain state: {}", e);
    }

    let consensus_interface_handle = tokio::spawn(async move {
        consensus_interface.start().await;
    });

    let mut oracle_clone = oracle.clone();
    let deposit_monitor_handle = tokio::spawn(async move {
        oracle_clone.poll_new_transactions(vec![]).await;
    });

    let mut node_state = NodeState::new_from_config(
        &network_handle,
        config,
        &swarm.network_events,
        deposit_intent_tx,
        oracle.clone(),
        TaprootWallet::new_with_db(
            oracle.clone(),
            Vec::new(),
            {
                let is_testnet: bool = std::env::var("IS_TESTNET")
                    .unwrap_or_else(|_| String::from("false"))
                    .parse()
                    .unwrap_or(false);

                if is_testnet {
                    BitcoinNetwork::Testnet
                } else {
                    BitcoinNetwork::Bitcoin
                }
            },
            db_arc.clone(),
        ),
        chain_message_tx,
        consensus_message_tx,
    )
    .await
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

    // Create shutdown signal handler for Docker compatibility
    let shutdown_signal = async {
        #[cfg(unix)]
        {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install SIGTERM handler");

            tokio::select! {
                _ = signal::ctrl_c() => {
                    tracing::info!("Received SIGINT, shutting down gracefully...");
                }
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, shutting down gracefully...");
                }
            }
        }

        #[cfg(not(unix))]
        {
            signal::ctrl_c().await.expect("Failed to listen for ctrl-c");
            tracing::info!("Received SIGINT, shutting down gracefully...");
        }
    };

    // Wait for shutdown signal or task completion
    tokio::select! {
        () = shutdown_signal => {
            // Shutdown signal received
        }
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
        result = chain_interface_handle => {
            match result {
                Ok(()) => tracing::info!("Chain interface stopped"),
                Err(e) => tracing::error!("Chain interface error: {}", e),
            }
        }
        result = metrics_server_handle => {
            match result {
                Ok(()) => tracing::info!("Metrics server stopped"),
                Err(e) => tracing::error!("Metrics server error: {}", e),
            }
        }
        result = consensus_interface_handle => {
            match result {
                Ok(()) => tracing::info!("Consensus interface stopped"),
                Err(e) => tracing::error!("Consensus interface error: {}", e),
            }
        }
    }

    Ok(())
}
