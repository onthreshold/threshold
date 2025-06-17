use crate::{
    NodeState,
    db::Db,
    handlers::Handler,
    handlers::consensus::{ConsensusPhase, ConsensusState},
    swarm_manager::Network,
    wallet::Wallet,
};
use libp2p::PeerId;
use tracing::info;
use types::network_event::NetworkEvent;

#[async_trait::async_trait]
impl<N: Network, D: Db, W: Wallet> Handler<N, D, W> for ConsensusState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), types::errors::NodeError> {
        if let Some(start_time) = self.round_start_time {
            if start_time.elapsed() >= self.round_timeout && self.is_leader {
                info!("Round timeout reached. I am current leader. Proposing new round.");
                let next_round = self.current_round + 1;
                let new_round_message = types::consensus::ConsensusMessage::NewRound(next_round);
                let data = types::consensus::ConsensusMessage::encode(&new_round_message).map_err(
                    |e| {
                        types::errors::NodeError::Error(format!(
                            "Failed to encode new round message: {e}"
                        ))
                    },
                )?;

                node.network_handle
                    .send_broadcast(self.leader_topic.clone(), data)
                    .map_err(|e| {
                        types::errors::NodeError::Error(format!(
                            "Failed to broadcast new round message: {e:?}"
                        ))
                    })?;
            }
        }

        if let Some(message) = message {
            match message {
                NetworkEvent::Subscribed { peer_id, topic } => {
                    if topic == self.leader_topic.hash() {
                        self.validators.insert(peer_id);

                        if self.current_round == 0 {
                            self.start_new_round(node)?;
                        }
                    }
                }
                NetworkEvent::GossipsubMessage(message) => {
                    if let Some(peer) = message.source {
                        if message.topic == self.leader_topic.hash() {
                            let consensus_message: types::consensus::ConsensusMessage =
                                types::consensus::ConsensusMessage::decode(&message.data).map_err(
                                    |e| {
                                        types::errors::NodeError::Error(format!(
                                            "Failed to decode consensus message: {}",
                                            e
                                        ))
                                    },
                                )?;

                            match consensus_message {
                                types::consensus::ConsensusMessage::LeaderAnnouncement(
                                    announcement,
                                ) => {
                                    self.handle_leader_announcement(node, announcement)?;
                                }
                                types::consensus::ConsensusMessage::NewRound(round) => {
                                    self.handle_new_round(node, peer, round).await?;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl ConsensusState {
    fn handle_new_round<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &NodeState<N, D, W>,
        sender: PeerId,
        round: u32,
    ) -> Result<(), types::errors::NodeError> {
        if round <= self.current_round {
            return Ok(()); // Old or current round, ignore
        }

        // Verify sender is the leader of the previous round
        if let Some(expected_leader) = self.proposer {
            if expected_leader != sender {
                info!("Ignoring NewRound message from non-leader {}", sender);
                return Ok(());
            }
        } else if self.current_round > 0 {
            // We should have a leader for any round > 0
            return Ok(());
        }

        self.current_round = round - 1;
        self.start_new_round(node)
    }

    fn start_new_round<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &NodeState<N, D, W>,
    ) -> Result<(), types::errors::NodeError> {
        self.current_round += 1;

        if let Some(new_leader) = self.select_leader(self.current_round) {
            self.proposer = Some(new_leader);
            self.is_leader = new_leader == node.peer_id;

            if self.is_leader {
                let announcement = types::consensus::LeaderAnnouncement {
                    leader: new_leader.to_bytes(),
                    round: self.current_round,
                };
                let message = types::consensus::ConsensusMessage::LeaderAnnouncement(announcement);

                let leader_data =
                    types::consensus::ConsensusMessage::encode(&message).map_err(|e| {
                        types::errors::NodeError::Error(format!("Failed to encode leader: {}", e))
                    })?;

                node.network_handle
                    .send_broadcast(self.leader_topic.clone(), leader_data)
                    .map_err(|e| {
                        types::errors::NodeError::Error(format!("Failed to publish leader: {e:?}"))
                    })?;
            }

            info!(
                "Round {} started with leader {}",
                self.current_round, new_leader
            );
        }

        self.current_state = ConsensusPhase::WaitingForPropose;
        self.round_start_time = Some(tokio::time::Instant::now());

        Ok(())
    }

    fn handle_leader_announcement<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
        announcement: types::consensus::LeaderAnnouncement,
    ) -> Result<(), types::errors::NodeError> {
        let leader = PeerId::from_bytes(&announcement.leader).map_err(|e| {
            types::errors::NodeError::Error(format!("Failed to decode leader bytes: {e}"))
        })?;

        if announcement.round >= self.current_round {
            self.current_round = announcement.round;
            self.proposer = Some(leader);
            self.is_leader = leader == node.peer_id;
            self.current_state = ConsensusPhase::WaitingForPropose;
            self.round_start_time = Some(tokio::time::Instant::now());

            info!(
                "Agreed on leader for round {} is {}",
                self.current_round, leader
            );
        }

        Ok(())
    }
}
