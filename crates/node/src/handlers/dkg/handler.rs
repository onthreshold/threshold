use crate::{NodeState, handlers::Handler, handlers::dkg::DkgState, wallet::Wallet};
use types::broadcast::BroadcastMessage;
use types::network::network_event::{DirectMessage, NetworkEvent};
use types::network::network_protocol::Network;
use types::proto::{ProtoDecode};
use types::proto::p2p_proto;

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for DkgState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            Some(NetworkEvent::Subscribed { peer_id, topic }) => {
                if topic == libp2p::gossipsub::IdentTopic::new("broadcast").hash() {
                    self.dkg_listeners.insert(peer_id);
                    self.round1_listeners.insert(peer_id);
                }
            }
            Some(NetworkEvent::GossipsubMessage(message)) => {
                if message.topic == libp2p::gossipsub::IdentTopic::new("broadcast").hash() {
                    if let Ok(BroadcastMessage::Dkg(gossip_msg)) = BroadcastMessage::decode(&message.data) {
                        if let Some(source_peer) = message.source {
                            // Determine inner DKG message variant
                            if let Some(p2p_proto::gossipsub_message::Message::Dkg(inner_dkg)) = gossip_msg.message {
                                use p2p_proto::dkg_message::Message as DkgInner;
                                match inner_dkg.message {
                                    Some(DkgInner::StartDkg(_)) => {
                                        self.handle_dkg_start(node)?;
                                    }
                                    Some(DkgInner::Round1Package(_)) => {
                                        self.handle_round1_payload(node, source_peer, &message.data)?;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            Some(NetworkEvent::MessageEvent((peer, DirectMessage::Round2Package(package)))) => {
                self.handle_round2_payload(node, peer, package).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
