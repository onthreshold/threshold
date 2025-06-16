use crate::swarm_manager::Network;
use crate::{NodeState, db::Db, handlers::Handler, handlers::dkg::DkgState, wallet::Wallet};
use types::network_event::{DirectMessage, NetworkEvent};

#[async_trait::async_trait]
impl<N: Network, D: Db, W: Wallet> Handler<N, D, W> for DkgState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            Some(NetworkEvent::Subscribed { peer_id, topic }) => {
                if topic == self.start_dkg_topic.hash() {
                    self.dkg_listeners.insert(peer_id);
                    tracing::trace!(
                        "Peer {} subscribed to topic {topic}. Listeners: {}",
                        peer_id,
                        self.dkg_listeners.len()
                    );
                    if let Err(e) = self.handle_dkg_start(node).await {
                        tracing::error!("❌ Failed to handle DKG start: {}", e);
                    }
                }
            }
            Some(NetworkEvent::GossipsubMessage(message)) => {
                if message.topic == self.round1_topic.hash() {
                    if let Some(source_peer) = message.source {
                        self.handle_round1_payload(node, source_peer, &message.data)?;
                    }
                }
            }
            Some(NetworkEvent::MessageEvent((peer, DirectMessage::Round2Package(package)))) => {
                self.handle_round2_payload(node, peer, package)?;
            }
            _ => {}
        }
        Ok(())
    }
}
