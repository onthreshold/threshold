mod node;
mod swarm_manager;

use crate::node::NodeState;
use swarm_manager::build_swarm;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let max_signers = 5;
    let min_signers = 3;

    let mut swarm = build_swarm()
        .map_err(|e| println!("Failed to build swarm {}", e.message))
        .expect("Failed to build swarm");

    let mut node_state = NodeState::new(&mut swarm, min_signers, max_signers);
    let _ = node_state.main_loop().await;

    Ok(())
}
