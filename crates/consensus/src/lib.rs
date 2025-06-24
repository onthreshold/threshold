use libp2p::{PeerId, gossipsub::IdentTopic};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::Instant;
use types::consensus::Vote;

pub mod consensus_interface;
pub mod main_loop;

pub use consensus_interface::{ConsensusInterface, ConsensusInterfaceImpl};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusPhase {
    WaitingForPropose,
    Propose,
    Prevote,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    StartNewRound {
        round: u32,
    },
    HandleVote {
        sender: Vec<u8>,
        vote: Vote,
    },
    HandleNewRound {
        sender: Vec<u8>,
        round: u32,
    },
    HandleLeaderAnnouncement {
        sender: Vec<u8>,
        leader: Vec<u8>,
        round: u32,
    },
    HandleBlockProposal {
        sender: Vec<u8>,
        raw_block: Vec<u8>,
    },
    TriggerConsensusRound {
        force_round: bool,
    },
    SetLeader {
        is_leader: bool,
    },
    AddValidator {
        peer_id: Vec<u8>,
    },
    GetConsensusState,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ConsensusResponse {
    StartNewRound {
        error: Option<String>,
    },
    HandleVote {
        error: Option<String>,
    },
    HandleNewRound {
        error: Option<String>,
    },
    HandleLeaderAnnouncement {
        error: Option<String>,
    },
    HandleBlockProposal {
        error: Option<String>,
    },
    TriggerConsensusRound {
        success: bool,
        message: String,
        round_number: u64,
    },
    SetLeader {
        error: Option<String>,
    },
    AddValidator {
        error: Option<String>,
    },
    GetConsensusState {
        phase: ConsensusPhase,
        round: u32,
        height: u64,
        is_leader: bool,
        validators_count: usize,
        prevotes_count: usize,
        precommits_count: usize,
    },
}

pub struct ConsensusState {
    pub current_state: ConsensusPhase,
    pub current_round: u32,
    pub current_height: u64,
    pub proposer: Option<PeerId>,
    pub validators: HashSet<PeerId>,

    pub broadcast_topic: IdentTopic,

    pub round_timeout: Duration,
    pub round_start_time: Option<Instant>,
    pub is_leader: bool,

    pub prevotes: HashSet<PeerId>,
    pub precommits: HashSet<PeerId>,
    pub current_block_hash: Option<Vec<u8>>,
    pub block_finalized: bool,
}

impl Default for ConsensusState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_state: ConsensusPhase::WaitingForPropose,
            current_round: 0,
            current_height: 0,
            proposer: None,
            validators: HashSet::new(),
            broadcast_topic: IdentTopic::new("broadcast"),
            round_timeout: Duration::from_secs(10),
            round_start_time: None,
            is_leader: false,
            prevotes: HashSet::new(),
            precommits: HashSet::new(),
            current_block_hash: None,
            block_finalized: false,
        }
    }

    #[must_use]
    pub fn select_leader(&self, round: u32) -> Option<PeerId> {
        if self.validators.is_empty() {
            return None;
        }

        let mut sorted_validators: Vec<PeerId> = self.validators.iter().copied().collect();
        sorted_validators.sort();

        let index = (round as usize) % self.validators.len();
        sorted_validators.get(index).copied()
    }
}

#[cfg(test)]
mod tests;
