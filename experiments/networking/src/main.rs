use libp2p::{identity, PeerId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    println!("Local peer id: {local_peer_id:?}");

    Ok(())
}
