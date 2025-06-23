use crate::{NodeState, handlers::Handler, handlers::dkg::DkgState, wallet::Wallet};
use p2p_proto::dkg_message::Message as DkgInner;
use types::broadcast::BroadcastMessage;
use types::network::network_event::{DirectMessage, NetworkEvent};
use types::network::network_protocol::Network;
use types::proto::ProtoDecode;
use types::proto::p2p_proto::{self, gossipsub_message::Message};

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for DkgState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            Some(NetworkEvent::Subscribed { peer_id, .. }) => {
                self.dkg_listeners.insert(peer_id);
                self.round1_listeners.insert(peer_id);
                self.handle_dkg_start(node)?;
            }
            Some(NetworkEvent::GossipsubMessage(message)) => {
                if let Ok(BroadcastMessage::Dkg(gossip_msg)) =
                    BroadcastMessage::decode(&message.data)
                {
                    if let Some(source_peer) = message.source {
                        if let Some(Message::Dkg(inner_dkg)) = gossip_msg.message {
                            match inner_dkg.message {
                                Some(DkgInner::StartDkg(_)) => {
                                    self.handle_dkg_start(node)?;
                                }
                                Some(DkgInner::Round1Package(_)) => {
                                    self.handle_round1_payload(node, source_peer, inner_dkg)?;
                                }
                                _ => {}
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
