use libp2p::{PeerId, identity::Keypair};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;

use node::{
    NodeState,
    grpc_service::NodeControlService,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Generate keypair for this node
    let keypair = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(keypair.public());

    println!("Starting node with peer ID: {}", local_peer_id);

    // Configure allowed peers (you can load this from config)
    let allowed_peers = vec![
        // Add your allowed peer IDs here
        // Example: "12D3KooWH3uVF6wv47WnArKHk5ZDVmGb6STjDdQJyeFgCgvQhBPF".parse().unwrap(),
    ];

    // Create node state
    let node_state = NodeState::new(
        keypair,
        allowed_peers,
        2, // min_signers
        3, // max_signers
    );

    // Wrap in Arc<Mutex> for shared access
    let node_state = Arc::new(Mutex::new(node_state));

    // Clone for gRPC server
    let grpc_node_state = Arc::clone(&node_state);

    // Spawn gRPC server in a separate task
    let grpc_handle = tokio::spawn(async move {
        let addr = "[::1]:50051".parse().unwrap();

        // Create the gRPC service
        let node_control_service = NodeControlService::new(grpc_node_state);

        println!("gRPC server listening on {}", addr);

        // Run the server
        Server::builder()
            .add_service(node_control_service.into_server())
            .serve(addr)
            .await
            .expect("gRPC server failed");
    });

    // Start listening on the swarm
    {
        let mut node_state_guard = node_state.lock().await;
        node_state_guard
            .swarm
            .listen_on("/ip4/0.0.0.0/tcp/0".parse()?)
            .expect("Failed to start listening");
    }

    // Run the main event loop
    let main_loop_handle = tokio::spawn(async move {
        let mut node_state_guard = node_state.lock().await;
        node_state_guard.main_loop().await
    });

    // Wait for either task to complete (they should run indefinitely)
    tokio::select! {
        result = grpc_handle => {
            match result {
                Ok(_) => println!("gRPC server stopped"),
                Err(e) => eprintln!("gRPC server error: {}", e),
            }
        }
        result = main_loop_handle => {
            match result {
                Ok(Ok(_)) => println!("Main loop stopped"),
                Ok(Err(e)) => eprintln!("Main loop error: {}", e),
                Err(e) => eprintln!("Main loop task error: {}", e),
            }
        }
    }

    Ok(())
}
