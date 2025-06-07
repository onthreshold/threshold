use tracing::error;

use crate::{
    dkg::DkgState,
    handler::Handler,
    swarm_manager::{DirectMessage, HandlerMessage, Network},
};

#[async_trait::async_trait]
impl<N: Network> Handler<N> for DkgState {
    async fn handle(
        &mut self,
        message: Option<HandlerMessage>,
        network_handle: &N,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            Some(HandlerMessage::Subscribed { peer_id, topic }) => {
                if topic == self.start_dkg_topic.hash() {
                    self.dkg_listeners.insert(peer_id);
                    println!(
                        "Peer {} subscribed to topic {topic}. Listeners: {}",
                        peer_id,
                        self.dkg_listeners.len()
                    );
                    if let Err(e) = self.handle_dkg_start(network_handle).await {
                        error!("❌ Failed to handle DKG start: {}", e);
                    }
                }
            }
            Some(HandlerMessage::GossipsubMessage(message)) => {
                if message.topic == self.round1_topic.hash() {
                    if let Some(source_peer) = message.source {
                        self.handle_round1_payload(network_handle, source_peer, &message.data)?;
                    }
                }
            }
            Some(HandlerMessage::MessageEvent((peer, DirectMessage::Round2Package(package)))) => {
                self.handle_round2_payload(network_handle, peer, package)?;
            }
            _ => {}
        }
        Ok(())
    }
}
