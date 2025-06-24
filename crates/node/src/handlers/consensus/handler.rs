use crate::{NodeState, handlers::Handler, handlers::consensus::ConsensusState, wallet::Wallet};
use consensus::{ConsensusMessage, ConsensusResponse};
use tracing::error;
use types::broadcast::BroadcastMessage;
use types::consensus::ConsensusMessage as ConsensusNetMessage;
use types::errors::NodeError;
use types::network::network_event::{NetworkEvent, SelfRequest, SelfResponse};
use types::network::network_protocol::Network;
use types::proto::ProtoDecode;

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for ConsensusState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: NetworkEvent,
    ) -> Result<(), NodeError> {
        match message {
            NetworkEvent::SelfRequest {
                request: SelfRequest::TriggerConsensusRound { force_round },
                response_channel,
            } => {
                let consensus_response = node
                    .consensus_interface_tx
                    .send_message_with_response(ConsensusMessage::TriggerConsensusRound {
                        force_round,
                    })
                    .await?;

                if let Some(response_channel) = response_channel {
                    if let ConsensusResponse::TriggerConsensusRound {
                        success,
                        message,
                        round_number,
                    } = consensus_response
                    {
                        response_channel
                            .send(SelfResponse::TriggerConsensusRoundResponse {
                                success,
                                message,
                                round_number,
                            })
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send response: {e}"))
                            })?;
                    } else {
                        error!("Unexpected response from consensus interface");
                        response_channel
                            .send(SelfResponse::TriggerConsensusRoundResponse {
                                success: false,
                                message: "Unexpected response from consensus interface".to_string(),
                                round_number: 0,
                            })
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send response: {e}"))
                            })?;
                    }
                }
            }
            NetworkEvent::Subscribed { peer_id, topic: _ } => {
                // Notify consensus about new validator
                let _ = node
                    .consensus_interface_tx
                    .send_message_with_response(ConsensusMessage::AddValidator {
                        peer_id: peer_id.to_bytes(),
                    })
                    .await;
            }
            NetworkEvent::GossipsubMessage(message) => {
                if let Some(peer) = message.source {
                    let broadcast = BroadcastMessage::decode(&message.data).map_err(|e| {
                        NodeError::Error(format!("Failed to decode broadcast message: {e}"))
                    })?;

                    match broadcast {
                        BroadcastMessage::Consensus(consensus_message) => match consensus_message {
                            ConsensusNetMessage::LeaderAnnouncement(announcement) => {
                                let _ = node
                                    .consensus_interface_tx
                                    .send_message_with_response(
                                        ConsensusMessage::HandleLeaderAnnouncement {
                                            sender: peer.to_bytes(),
                                            leader: announcement.leader,
                                            round: announcement.round,
                                        },
                                    )
                                    .await;
                            }
                            ConsensusNetMessage::NewRound(round) => {
                                let _ = node
                                    .consensus_interface_tx
                                    .send_message_with_response(ConsensusMessage::HandleNewRound {
                                        sender: peer.to_bytes(),
                                        round,
                                    })
                                    .await;
                            }
                            ConsensusNetMessage::Vote(vote) => {
                                let _ = node
                                    .consensus_interface_tx
                                    .send_message_with_response(ConsensusMessage::HandleVote {
                                        sender: peer.to_bytes(),
                                        vote,
                                    })
                                    .await;
                            }
                            ConsensusNetMessage::BlockProposal { proposer, raw_block } => {
                                let _ = node
                                    .consensus_interface_tx
                                    .send_message_with_response(ConsensusMessage::HandleBlockProposal {
                                        sender: proposer,
                                        raw_block,
                                    })
                                    .await;
                            }
                        },
                        BroadcastMessage::Block(raw_block) => {
                            let _ = node
                                .consensus_interface_tx
                                .send_message_with_response(ConsensusMessage::HandleBlockProposal {
                                    sender: peer.to_bytes(),
                                    raw_block,
                                })
                                .await;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}
