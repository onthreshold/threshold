use crate::{
    NodeState,
    handlers::Handler,
    handlers::consensus::{ConsensusPhase, ConsensusState},
    wallet::Wallet,
};
use abci::{ChainMessage, ChainResponse};
use libp2p::PeerId;
use protocol::block::Block;
use sha2::{Digest, Sha256};
use tracing::info;
use types::broadcast::BroadcastMessage;
use types::consensus::{ConsensusMessage, LeaderAnnouncement, Vote};
use types::errors::NodeError;
use types::network::network_protocol::Network;
use types::{
    consensus::VoteType,
    network::network_event::{NetworkEvent, SelfRequest, SelfResponse},
    proto::ProtoDecode,
};

#[derive(Clone)]
struct RawBlockBytes(Vec<u8>);

impl types::proto::ProtoEncode for RawBlockBytes {
    fn encode(&self) -> Result<Vec<u8>, String> {
        Ok(self.0.clone())
    }
}

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for ConsensusState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        if self.is_leader && self.current_state == ConsensusPhase::WaitingForPropose {
            if let Ok(ChainResponse::GetProposedBlock { block }) = node
                .chain_interface_tx
                .send_message_with_response(ChainMessage::GetProposedBlock {
                    previous_block: None,
                    proposer: node.peer_id.to_bytes(),
                })
                .await
            {
                if !block.body.transactions.is_empty() {
                    let bytes = block.serialize()?;

                    let broadcast_msg = BroadcastMessage::Block(bytes);

                    node.network_handle
                        .send_broadcast(self.broadcast_topic.clone(), broadcast_msg)
                        .map_err(|e| {
                            NodeError::Error(format!("Failed to broadcast block proposal: {e:?}"))
                        })?;

                    info!(
                        "📦 Proposed block for round {} with {} txs",
                        self.current_round,
                        block.body.transactions.len()
                    );

                    self.current_state = ConsensusPhase::Propose;
                }
            }
        }

        if let Some(start_time) = self.round_start_time {
            if start_time.elapsed() >= self.round_timeout && self.is_leader {
                info!("Round timeout reached. I am current leader. Proposing new round.");
                let next_round = self.current_round + 1;
                let new_round_message = ConsensusMessage::NewRound(next_round);

                node.network_handle
                    .send_broadcast(
                        self.broadcast_topic.clone(),
                        BroadcastMessage::Consensus(new_round_message),
                    )
                    .map_err(|e| {
                        NodeError::Error(format!("Failed to broadcast new round message: {e:?}"))
                    })?;
            }
        }

        if let Some(message) = message {
            match message {
                NetworkEvent::SelfRequest {
                    request: SelfRequest::TriggerConsensusRound { force_round },
                    response_channel,
                } => {
                    self.start_new_round(node)?;
                    let round_number = self.current_round;

                    if let Some(response_channel) = response_channel {
                        response_channel
                            .send(SelfResponse::TriggerConsensusRoundResponse {
                                success: true,
                                message: if force_round {
                                    "Forced consensus round triggered".to_string()
                                } else {
                                    "Consensus round triggered".to_string()
                                },
                                round_number: u64::from(round_number),
                            })
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send response: {e}"))
                            })?;
                    }
                }
                NetworkEvent::Subscribed { peer_id, topic } => {
                    if topic == self.broadcast_topic.hash() {
                        self.validators.insert(peer_id);

                        if self.current_round == 0 {
                            self.start_new_round(node)?;
                        }
                    }
                }
                NetworkEvent::GossipsubMessage(message) => {
                    if let Some(peer) = message.source {
                        if message.topic != self.broadcast_topic.hash() {
                            return Ok(());
                        }

                        let broadcast = BroadcastMessage::decode(&message.data).map_err(|e| {
                            NodeError::Error(format!("Failed to decode broadcast message: {e}"))
                        })?;

                        match broadcast {
                            BroadcastMessage::Consensus(consensus_message) => match consensus_message {
                                ConsensusMessage::LeaderAnnouncement(announcement) => {
                                    self.handle_leader_announcement(node, &announcement)?;
                                }
                                ConsensusMessage::NewRound(round) => {
                                    self.handle_new_round(node, peer, round)?;
                                }
                                ConsensusMessage::Vote(vote) => {
                                    self.handle_vote(node, peer, &vote).await;
                                }
                            },
                            BroadcastMessage::Block(raw_block) => {
                                match Block::deserialize(&raw_block) {
                                    Ok(block) => {
                                        info!(
                                            "📥 Received block proposal for round {} from {} with {} txs",
                                            self.current_round,
                                            peer,
                                            block.body.transactions.len()
                                        );
                                        let Ok(ChainResponse::GetProposedBlock { block: local_block }) =
                                            node.chain_interface_tx
                                                .send_message_with_response(
                                                    ChainMessage::GetProposedBlock {
                                                        previous_block: None,
                                                        proposer: node.peer_id.to_bytes(),
                                                    },
                                                )
                                                .await
                                        else {
                                            return Err(NodeError::Error(
                                                "Failed to get proposed block".to_string(),
                                            ));
                                        };

                                    // Validate block by comparing transactions rather than entire block
                                    // (timestamps and proposers will be different for each node)
                                    if local_block.body.transactions == block.body.transactions {
                                        info!("Block is valid. Sending prevote.");
                                        self.send_vote(node, &block, VoteType::Prevote)?;
                                    } else {
                                        info!(
                                            "Block is invalid. Not voting - transaction mismatch"
                                        );
                                        info!(
                                            "Local txs: {}, Received txs: {}",
                                            local_block.body.transactions.len(),
                                            block.body.transactions.len()
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to deserialize proposed block from {}: {}",
                                        peer,
                                        e
                                    );
                                }
                            }
                        }

                        if message.topic == self.vote_topic.hash() {
                            let vote_message: ConsensusMessage =
                                ConsensusMessage::decode(&message.data).map_err(|e| {
                                    NodeError::Error(format!("Failed to decode vote message: {e}"))
                                })?;

                            if let ConsensusMessage::Vote(vote) = vote_message {
                                self.handle_vote(node, peer, &vote).await;
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
    fn handle_new_round<N: Network, W: Wallet>(
        &mut self,
        node: &NodeState<N, W>,
        sender: PeerId,
        round: u32,
    ) -> Result<(), NodeError> {
        if round <= self.current_round {
            return Ok(());
        }

        if let Some(expected_leader) = self.proposer {
            if expected_leader != sender {
                info!("Ignoring NewRound message from non-leader {}", sender);
                return Ok(());
            }
        } else if self.current_round > 0 {
            return Ok(());
        }

        self.current_round = round - 1;
        self.start_new_round(node)
    }

    fn start_new_round<N: Network, W: Wallet>(
        &mut self,
        node: &NodeState<N, W>,
    ) -> Result<(), NodeError> {
        self.current_round += 1;

        if let Some(new_leader) = self.select_leader(self.current_round) {
            self.proposer = Some(new_leader);
            self.is_leader = new_leader == node.peer_id;

            if self.is_leader {
                let announcement = LeaderAnnouncement {
                    leader: new_leader.to_bytes(),
                    round: self.current_round,
                };
                let message = ConsensusMessage::LeaderAnnouncement(announcement);

                node.network_handle
                    .send_broadcast(
                        self.broadcast_topic.clone(),
                        BroadcastMessage::Consensus(message),
                    )
                    .map_err(|e| NodeError::Error(format!("Failed to publish leader: {e:?}")))?;
            }

            info!(
                "Round {} started with leader {}",
                self.current_round, new_leader
            );
        }

        self.current_state = ConsensusPhase::WaitingForPropose;
        self.round_start_time = Some(tokio::time::Instant::now());

        self.prevotes.clear();
        self.current_block_hash = None;

        Ok(())
    }

    fn handle_leader_announcement<N: Network, W: Wallet>(
        &mut self,
        node: &NodeState<N, W>,
        announcement: &LeaderAnnouncement,
    ) -> Result<(), NodeError> {
        let leader = PeerId::from_bytes(&announcement.leader)
            .map_err(|e| NodeError::Error(format!("Failed to decode leader bytes: {e}")))?;

        if announcement.round >= self.current_round {
            self.current_round = announcement.round;
            self.proposer = Some(leader);
            self.is_leader = leader == node.peer_id;
            self.current_state = ConsensusPhase::WaitingForPropose;
            self.round_start_time = Some(tokio::time::Instant::now());

            info!(
                "Agreed on leader for round {} is {}",
                self.current_round,
                node.network_handle.peer_name(&leader)
            );
        }

        Ok(())
    }

    fn send_vote(
        &self,
        node: &NodeState<impl Network, impl Wallet>,
        block: &Block,
        vote_type: VoteType,
    ) -> Result<(), NodeError> {
        let block_bytes = block.serialize()?;
        let mut hasher = Sha256::new();
        hasher.update(&block_bytes);
        let block_hash = hasher.finalize().to_vec();

        let vote_type_clone = vote_type.clone();
        let vote = Vote {
            round: self.current_round,
            height: self.current_height,
            block_hash: block_hash.clone(),
            voter: node.peer_id.to_bytes(),
            vote_type,
        };

        let vote_message = ConsensusMessage::Vote(vote);

        node.network_handle
            .send_broadcast(
                self.broadcast_topic.clone(),
                BroadcastMessage::Consensus(vote_message),
            )
            .map_err(|e| NodeError::Error(format!("Failed to broadcast vote: {e:?}")))?;

        info!(
            "Sending {:?} vote for block hash {:?} in round {}",
            vote_type_clone, block_hash, self.current_round
        );

        Ok(())
    }

    async fn handle_vote(
        &mut self,
        node: &mut NodeState<impl Network, impl Wallet>,
        sender: PeerId,
        vote: &Vote,
    ) {
        if vote.round != self.current_round || vote.height != self.current_height {
            return;
        }

        if !self.validators.contains(&sender) {
            info!("Ignoring vote from non-validator {}", sender);
            return;
        }

        match vote.vote_type {
            VoteType::Prevote => {
                if self.prevotes.insert(sender) {
                    info!(
                        "Got prevote from {} for block hash {:?}. Total: {}/{}",
                        node.network_handle.peer_name(&sender),
                        vote.block_hash,
                        self.prevotes.len(),
                        self.validators.len()
                    );

                    if self.prevotes.len() > (self.validators.len() * 2) / 3 {
                        info!("Got 2/3+ prevotes. Sending precommit vote.");

                        let vote = Vote {
                            round: self.current_round,
                            height: self.current_height,
                            block_hash: vote.block_hash.clone(),
                            voter: node.peer_id.to_bytes(),
                            vote_type: VoteType::Precommit,
                        };

                        let vote_message = ConsensusMessage::Vote(vote);
                        node.network_handle
                            .send_broadcast(
                                self.broadcast_topic.clone(),
                                BroadcastMessage::Consensus(vote_message),
                            )
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send precommit vote: {e:?}"))
                            })
                            .ok();
                    }
                }
            }
            VoteType::Precommit => {
                if self.precommits.insert(sender) {
                    info!(
                        "Got precommit from {} for block hash {:?}. Total: {}/{}",
                        node.network_handle.peer_name(&sender),
                        vote.block_hash,
                        self.precommits.len(),
                        self.validators.len()
                    );

                    if self.precommits.len() > (self.validators.len() * 2) / 3 {
                        info!("Got 2/3+ precommits. Finalizing block...");

                        // Get the proposed block to finalize
                        let Ok(ChainResponse::GetProposedBlock { block }) = node
                            .chain_interface_tx
                            .send_message_with_response(ChainMessage::GetProposedBlock {
                                previous_block: None,
                                proposer: node.peer_id.to_bytes(),
                            })
                            .await
                        else {
                            tracing::error!("Failed to get proposed block for finalization");
                            return;
                        };

                        // Finalize and store the block
                        match node
                            .chain_interface_tx
                            .send_message_with_response(ChainMessage::FinalizeBlock {
                                block: block.clone(),
                            })
                            .await
                        {
                            Ok(ChainResponse::FinalizeAndStoreBlock { error: None }) => {
                                info!(
                                    "🎉 Successfully finalized block at height {} with {} transactions",
                                    block.header.height,
                                    block.body.transactions.len()
                                );

                                self.start_new_round(node).ok();
                            }
                            Ok(ChainResponse::FinalizeAndStoreBlock { error: Some(e) }) => {
                                tracing::error!("Failed to finalize block: {}", e);
                            }
                            _ => {
                                tracing::error!("Unexpected response when finalizing block");
                            }
                        }
                    }
                }
            }
        }
    }
}
