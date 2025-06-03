use node::NodeState;
use node::swarm_manager::build_swarm;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    let max_signers = args.get(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(5);

    let min_signers = args.get(2).and_then(|s| s.parse::<u16>().ok()).unwrap_or(3);

    println!(
        "Starting node with max_signers={}, min_signers={}",
        max_signers, min_signers
    );

    let mut swarm = build_swarm().map_err(|e| format!("Failed to build swarm: {}", e.message))?;

    let mut node_state = NodeState::new(&mut swarm, min_signers, max_signers);
    node_state.main_loop().await?;

    Ok(())
}
