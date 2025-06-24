use crate::{ConsensusMessage, ConsensusPhase, ConsensusResponse, ConsensusState};
use libp2p::PeerId;
use protocol::block::Block;
use sha2::{Digest, Sha256};
use tracing::{debug, error, info, warn};
use types::broadcast::BroadcastMessage;
use types::consensus::{
    ConsensusMessage as ConsensusNetMessage, LeaderAnnouncement, Vote, VoteType,
};
use types::current_round_metrics;
use types::errors::NodeError;

#[async_trait::async_trait]
pub trait ConsensusInterface: Send + Sync {
    async fn handle_message(&mut self, message: ConsensusMessage) -> ConsensusResponse;
}

pub struct ConsensusInterfaceImpl {
    pub state: ConsensusState,
    pub message_stream: messenger::Reciver<ConsensusMessage, ConsensusResponse>,
    pub network_events_tx:
        Option<tokio::sync::broadcast::Sender<types::network::network_event::NetworkEvent>>,
    pub chain_interface_tx: Option<messenger::Sender<abci::ChainMessage, abci::ChainResponse>>,
    pub peer_id: Option<PeerId>,
    pub max_validators: Option<usize>, // Expected number of validators
}

impl ConsensusInterfaceImpl {
    #[must_use]
    pub fn new() -> (Self, messenger::Sender<ConsensusMessage, ConsensusResponse>) {
        let (tx, rx) = messenger::channel(100, Some(100));
        (
            Self {
                state: ConsensusState::new(),
                message_stream: rx,
                network_events_tx: None,
                chain_interface_tx: None,
                peer_id: None,
                max_validators: None,
            },
            tx,
        )
    }

    pub fn set_network_events_tx(
        &mut self,
        sender: tokio::sync::broadcast::Sender<types::network::network_event::NetworkEvent>,
    ) {
        self.network_events_tx = Some(sender);
    }

    pub fn set_chain_interface(
        &mut self,
        tx: messenger::Sender<abci::ChainMessage, abci::ChainResponse>,
    ) {
        self.chain_interface_tx = Some(tx);
    }

    pub const fn set_peer_id(&mut self, peer_id: PeerId) {
        self.peer_id = Some(peer_id);
    }

    pub const fn set_max_validators(&mut self, max_validators: usize) {
        self.max_validators = Some(max_validators);
    }

    fn send_broadcast(&self, message: BroadcastMessage) -> Result<(), NodeError> {
        if let Some(sender) = &self.network_events_tx {
            sender
                .send(types::network::network_event::NetworkEvent::SendBroadcast { message })
                .map_err(|e| NodeError::Error(format!("Failed to send network event: {e}")))?;
        }
        Ok(())
    }

    async fn get_proposed_block(&mut self, proposer: Vec<u8>) -> Result<Block, NodeError> {
        if let Some(chain_tx) = &mut self.chain_interface_tx {
            match chain_tx
                .send_message_with_response(abci::ChainMessage::GetProposedBlock {
                    previous_block: None,
                    proposer,
                })
                .await
            {
                Ok(abci::ChainResponse::GetProposedBlock { block }) => Ok(block),
                Ok(_) => Err(NodeError::Error(
                    "Unexpected response from chain interface".to_string(),
                )),
                Err(e) => Err(e),
            }
        } else {
            Err(NodeError::Error(
                "Chain interface not available".to_string(),
            ))
        }
    }

    async fn finalize_block(&mut self, block: Block) -> Result<(), NodeError> {
        if let Some(chain_tx) = &mut self.chain_interface_tx {
            match chain_tx
                .send_message_with_response(abci::ChainMessage::FinalizeBlock { block })
                .await
            {
                Ok(abci::ChainResponse::FinalizeAndStoreBlock { error: None }) => Ok(()),
                Ok(abci::ChainResponse::FinalizeAndStoreBlock { error: Some(e) }) => {
                    Err(NodeError::Error(format!("Failed to finalize block: {e}")))
                }
                Ok(_) => Err(NodeError::Error(
                    "Unexpected response from chain interface".to_string(),
                )),
                Err(e) => Err(e),
            }
        } else {
            Err(NodeError::Error(
                "Chain interface not available".to_string(),
            ))
        }
    }

    pub fn start_new_round(&mut self) -> Result<(), NodeError> {
        self.state.current_round += 1;

        if let Some(peer_id) = self.peer_id {
            current_round_metrics!(self.state.current_round, peer_id.to_string());
        }

        if let Some(new_leader) = self.state.select_leader(self.state.current_round) {
            self.state.proposer = Some(new_leader);
            self.state.is_leader = self.peer_id == Some(new_leader);

            if self.state.is_leader {
                let announcement = LeaderAnnouncement {
                    leader: new_leader.to_bytes(),
                    round: self.state.current_round,
                };
                let message = ConsensusNetMessage::LeaderAnnouncement(announcement);
                self.send_broadcast(BroadcastMessage::Consensus(message))?;
            }

            info!(
                "Round {} started with leader {}",
                self.state.current_round, new_leader
            );
        }

        self.state.current_state = ConsensusPhase::WaitingForPropose;
        self.state.round_start_time = Some(tokio::time::Instant::now());

        self.state.prevotes.clear();
        self.state.precommits.clear();
        self.state.current_block_hash = None;
        self.state.block_finalized = false;

        debug!(
            "🔄 Cleared vote counts for new round {}. Validator set: {}",
            self.state.current_round,
            self.state.validators.len()
        );

        Ok(())
    }

    pub async fn propose_block_as_leader(&mut self) -> Result<(), NodeError> {
        debug!(
            "Proposing block as leader for round {}",
            self.state.current_round
        );

        // Get the proposed block from chain ilibp2p::PeerId::to_bytes
        let proposer_bytes = self
            .peer_id
            .map(libp2p::PeerId::to_bytes)
            .unwrap_or_default();
        let block = self.get_proposed_block(proposer_bytes).await?;

        // Serialize and broadcast the block proposal
        let raw_block = block.serialize()?;
        let proposal_message = ConsensusNetMessage::BlockProposal {
            proposer: self
                .peer_id
                .map(libp2p::PeerId::to_bytes)
                .unwrap_or_default(),
            raw_block,
        };

        self.send_broadcast(BroadcastMessage::Consensus(proposal_message))?;

        info!(
            "📤 Proposed block for round {} with {} transactions",
            self.state.current_round,
            block.body.transactions.len()
        );

        // Update our state to reflect that we've proposed
        self.state.current_state = ConsensusPhase::Prevote;

        Ok(())
    }

    async fn handle_block_proposal(
        &mut self,
        sender: PeerId,
        raw_block: Vec<u8>,
    ) -> Result<(), NodeError> {
        match Block::deserialize(&raw_block) {
            Ok(block) => {
                info!(
                    "📥 Received block proposal for round {} from {} with {} txs",
                    self.state.current_round,
                    sender,
                    block.body.transactions.len()
                );

                let proposer_bytes = self
                    .state
                    .proposer
                    .map(libp2p::PeerId::to_bytes)
                    .unwrap_or_default();
                let local_block = self.get_proposed_block(proposer_bytes).await?;

                if local_block == block {
                    info!("Block is valid. Sending prevote.");
                    self.send_vote(&block, &VoteType::Prevote)?;
                } else {
                    info!("Block is invalid. Not voting - transaction mismatch");
                    info!(
                        "Local txs: {:?}, Received txs: {:?}",
                        local_block.body.transactions, block.body.transactions
                    );
                }
            }
            Err(e) => warn!("Failed to deserialize block: {e}"),
        }
        Ok(())
    }

    fn send_vote(&self, block: &Block, vote_type: &VoteType) -> Result<(), NodeError> {
        let block_bytes = block.serialize()?;
        let mut hasher = Sha256::new();
        hasher.update(&block_bytes);
        let block_hash = hasher.finalize().to_vec();

        let vote = Vote {
            round: self.state.current_round,
            height: self.state.current_height,
            block_hash: block_hash.clone(),
            voter: self
                .peer_id
                .map(libp2p::PeerId::to_bytes)
                .unwrap_or_default(),
            vote_type: vote_type.clone(),
        };

        let vote_message = ConsensusNetMessage::Vote(vote);
        self.send_broadcast(BroadcastMessage::Consensus(vote_message))?;

        debug!(
            "🗳️  Sending {:?} vote for block hash {} in round {} from {} | validators: {}",
            vote_type,
            hex::encode(&block_hash[..8]),
            self.state.current_round,
            self.peer_id.map(|p| p.to_string()).unwrap_or_default(),
            self.state.validators.len()
        );

        Ok(())
    }

    fn process_prevote_vote(&mut self, sender: PeerId, vote: &Vote) {
        if self.state.prevotes.insert(sender) {
            debug!(
                "✅ Added prevote from {} for block hash {}. Total: {}/{} | Need: {}",
                sender,
                hex::encode(&vote.block_hash[..8]),
                self.state.prevotes.len(),
                self.state.validators.len(),
                (self.state.validators.len() * 2) / 3 + 1
            );

            if self.state.prevotes.len() >= (self.state.validators.len() * 2) / 3 {
                info!(
                    "🎯 Got 2/3+ prevotes ({}/{}). Sending precommit vote.",
                    self.state.prevotes.len(),
                    self.state.validators.len()
                );

                let vote = Vote {
                    round: self.state.current_round,
                    height: self.state.current_height,
                    block_hash: vote.block_hash.clone(),
                    voter: self
                        .peer_id
                        .map(libp2p::PeerId::to_bytes)
                        .unwrap_or_default(),
                    vote_type: VoteType::Precommit,
                };

                let vote_message = ConsensusNetMessage::Vote(vote);
                self.send_broadcast(BroadcastMessage::Consensus(vote_message))
                    .ok();
            }
        }
    }

    async fn process_precommit_vote(&mut self, sender: PeerId, vote: &Vote) {
        if self.state.precommits.insert(sender) {
            debug!(
                "✅ Added precommit from {} for block hash {}. Total: {}/{} | Need: {}",
                sender,
                hex::encode(&vote.block_hash[..8]),
                self.state.precommits.len(),
                self.state.validators.len(),
                (self.state.validators.len() * 2) / 3 + 1
            );

            if self.state.precommits.len() >= (self.state.validators.len() * 2) / 3 {
                if self.state.block_finalized {
                    debug!(
                        "⏭️  Block already finalized for this round, skipping duplicate finalization"
                    );
                } else {
                    info!(
                        "🎉 Got 2/3+ precommits ({}/{}). Finalizing block...",
                        self.state.precommits.len(),
                        self.state.validators.len()
                    );

                    self.state.block_finalized = true;

                    let proposer_bytes = self
                        .peer_id
                        .map(libp2p::PeerId::to_bytes)
                        .unwrap_or_default();
                    match self.get_proposed_block(proposer_bytes).await {
                        Ok(block) => match self.finalize_block(block.clone()).await {
                            Ok(()) => {
                                info!(
                                    "🎉 Successfully finalized block at height {} with {} transactions",
                                    block.header.height,
                                    block.body.transactions.len()
                                );

                                self.state.current_height = block.header.height;
                                info!(
                                    "✅ Updated consensus height to {}",
                                    self.state.current_height
                                );
                            }
                            Err(e) => error!("Failed to finalize block: {}", e),
                        },
                        Err(e) => error!("Failed to get proposed block for finalization: {}", e),
                    }
                }
            }
        }
    }

    async fn handle_vote(&mut self, sender: PeerId, vote: &Vote) {
        debug!(
            "📨 Received {:?} vote from {} for block hash {} | round: {} (current: {}), height: {} (current: {})",
            vote.vote_type,
            sender,
            hex::encode(&vote.block_hash[..8]),
            vote.round,
            self.state.current_round,
            vote.height,
            self.state.current_height
        );

        if !self.state.validators.contains(&sender) {
            warn!(
                "❌ Rejecting vote from {} - not in validator set (size: {})",
                sender,
                self.state.validators.len()
            );
            return;
        }

        match vote.vote_type {
            VoteType::Prevote => {
                self.process_prevote_vote(sender, vote);
            }
            VoteType::Precommit => {
                self.process_precommit_vote(sender, vote).await;
            }
        }
    }
}

#[async_trait::async_trait]
impl ConsensusInterface for ConsensusInterfaceImpl {
    async fn handle_message(&mut self, message: ConsensusMessage) -> ConsensusResponse {
        match message {
            ConsensusMessage::StartNewRound { round: _ } => match self.start_new_round() {
                Ok(()) => ConsensusResponse::StartNewRound { error: None },
                Err(e) => ConsensusResponse::StartNewRound {
                    error: Some(e.to_string()),
                },
            },
            ConsensusMessage::HandleVote { sender, vote } => match PeerId::from_bytes(&sender) {
                Ok(peer_id) => {
                    self.handle_vote(peer_id, &vote).await;
                    ConsensusResponse::HandleVote { error: None }
                }
                Err(e) => ConsensusResponse::HandleVote {
                    error: Some(format!("Failed to decode sender peer ID: {e}")),
                },
            },
            ConsensusMessage::HandleNewRound { sender, round } => {
                match PeerId::from_bytes(&sender) {
                    Ok(peer_id) => {
                        if round <= self.state.current_round {
                            return ConsensusResponse::HandleNewRound { error: None };
                        }

                        if let Some(expected_leader) = self.state.proposer {
                            if expected_leader != peer_id {
                                debug!("Ignoring NewRound message from non-leader {}", peer_id);
                                return ConsensusResponse::HandleNewRound { error: None };
                            }
                        } else if self.state.current_round > 0 {
                            return ConsensusResponse::HandleNewRound { error: None };
                        }

                        self.state.current_round = round - 1;
                        match self.start_new_round() {
                            Ok(()) => ConsensusResponse::HandleNewRound { error: None },
                            Err(e) => ConsensusResponse::HandleNewRound {
                                error: Some(e.to_string()),
                            },
                        }
                    }
                    Err(e) => ConsensusResponse::HandleNewRound {
                        error: Some(format!("Failed to decode sender peer ID: {e}")),
                    },
                }
            }
            ConsensusMessage::HandleLeaderAnnouncement {
                sender,
                leader,
                round,
            } => match (PeerId::from_bytes(&sender), PeerId::from_bytes(&leader)) {
                (Ok(_sender_id), Ok(leader_id)) => {
                    if round >= self.state.current_round {
                        self.state.current_round = round;
                        self.state.proposer = Some(leader_id);
                        self.state.is_leader = self.peer_id == Some(leader_id);
                        self.state.current_state = ConsensusPhase::WaitingForPropose;
                        self.state.round_start_time = Some(tokio::time::Instant::now());

                        debug!(
                            "Agreed on leader for round {} is {}",
                            self.state.current_round, leader_id
                        );
                    }
                    ConsensusResponse::HandleLeaderAnnouncement { error: None }
                }
                (Err(e), _) | (_, Err(e)) => ConsensusResponse::HandleLeaderAnnouncement {
                    error: Some(format!("Failed to decode peer ID: {e}")),
                },
            },
            ConsensusMessage::HandleBlockProposal { sender, raw_block } => {
                match PeerId::from_bytes(&sender) {
                    Ok(peer_id) => match self.handle_block_proposal(peer_id, raw_block).await {
                        Ok(()) => ConsensusResponse::HandleBlockProposal { error: None },
                        Err(e) => ConsensusResponse::HandleBlockProposal {
                            error: Some(e.to_string()),
                        },
                    },
                    Err(e) => ConsensusResponse::HandleBlockProposal {
                        error: Some(format!("Failed to decode sender peer ID: {e}")),
                    },
                }
            }
            ConsensusMessage::TriggerConsensusRound { force_round: _ } => {
                match self.start_new_round() {
                    Ok(()) => ConsensusResponse::TriggerConsensusRound {
                        success: true,
                        message: "Consensus round triggered".to_string(),
                        round_number: u64::from(self.state.current_round),
                    },
                    Err(e) => ConsensusResponse::TriggerConsensusRound {
                        success: false,
                        message: format!("Failed to trigger consensus round: {e}"),
                        round_number: u64::from(self.state.current_round),
                    },
                }
            }
            ConsensusMessage::SetLeader { is_leader } => {
                self.state.is_leader = is_leader;
                ConsensusResponse::SetLeader { error: None }
            }
            ConsensusMessage::AddValidator { peer_id } => {
                match PeerId::from_bytes(&peer_id) {
                    Ok(peer) => {
                        self.state.validators.insert(peer);
                        info!(
                            "🔗 Added validator. Validator set size: {}",
                            self.state.validators.len()
                        );

                        if self.state.current_round == 0 {
                            if let Some(max_validators) = self.max_validators {
                                if self.state.validators.len() >= max_validators {
                                    info!(
                                        "🚀 Reached {} validators, auto-starting consensus round",
                                        max_validators
                                    );
                                    match self.start_new_round() {
                                        Ok(()) => ConsensusResponse::AddValidator { error: None },
                                        Err(e) => ConsensusResponse::AddValidator {
                                            error: Some(e.to_string()),
                                        },
                                    }
                                } else {
                                    info!(
                                        "⏳ Waiting for more validators ({}/{})",
                                        self.state.validators.len(),
                                        max_validators
                                    );
                                    ConsensusResponse::AddValidator { error: None }
                                }
                            } else {
                                // No max validators set, don't auto-start
                                ConsensusResponse::AddValidator { error: None }
                            }
                        } else {
                            ConsensusResponse::AddValidator { error: None }
                        }
                    }
                    Err(e) => ConsensusResponse::AddValidator {
                        error: Some(format!("Failed to decode peer ID: {e}")),
                    },
                }
            }
            ConsensusMessage::GetConsensusState => ConsensusResponse::GetConsensusState {
                phase: self.state.current_state.clone(),
                round: self.state.current_round,
                height: self.state.current_height,
                is_leader: self.state.is_leader,
                validators_count: self.state.validators.len(),
                prevotes_count: self.state.prevotes.len(),
                precommits_count: self.state.precommits.len(),
            },
        }
    }
}
