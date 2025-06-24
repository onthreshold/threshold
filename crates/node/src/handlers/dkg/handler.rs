use crate::{NodeState, handlers::Handler, handlers::dkg::DkgState, wallet::Wallet};
use p2p_proto::dkg_message::Message as DkgInner;
use types::broadcast::BroadcastMessage;
use types::network::network_event::{DirectMessage, NetworkEvent};
use types::network::network_protocol::Network;
use types::proto::ProtoDecode;
use types::proto::p2p_proto::{self, gossipsub_message::Message};
use types::{dkg_round1_package_metrics, dkg_round2_package_metrics, dkg_start_metrics};

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for DkgState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: NetworkEvent,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            NetworkEvent::Subscribed { peer_id, .. } => {
                self.dkg_listeners.insert(peer_id);
                self.round1_listeners.insert(peer_id);
                self.handle_dkg_start(node)?;
            }
            NetworkEvent::GossipsubMessage(message) => {
                if let Ok(BroadcastMessage::Dkg(gossip_msg)) =
                    BroadcastMessage::decode(&message.data)
                {
                    if let Some(source_peer) = message.source {
                        if let Some(Message::Dkg(inner_dkg)) = gossip_msg.message {
                            match inner_dkg.message {
                                Some(DkgInner::StartDkg(_)) => {
                                    dkg_start_metrics!(node.network_handle.peer_name(&source_peer));
                                    self.handle_dkg_start(node)?;
                                }
                                Some(DkgInner::Round1Package(_)) => {
                                    dkg_round1_package_metrics!(
                                        node.network_handle.peer_name(&source_peer)
                                    );
                                    self.handle_round1_payload(node, source_peer, inner_dkg)?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            NetworkEvent::MessageEvent((peer, DirectMessage::Round2Package(package))) => {
                dkg_round2_package_metrics!(node.network_handle.peer_name(&peer));
                self.handle_round2_payload(node, peer, package).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
