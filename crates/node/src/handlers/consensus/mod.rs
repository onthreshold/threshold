use libp2p::{PeerId, gossipsub::IdentTopic};
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::Instant;

pub mod handler;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusPhase {
    WaitingForPropose,
    Propose,
    Prevote,
}

pub struct ConsensusState {
    pub current_state: ConsensusPhase,
    pub current_round: u32,
    pub current_height: u64,
    pub proposer: Option<PeerId>,
    pub validators: HashSet<PeerId>,

    pub leader_topic: IdentTopic,
    pub block_topic: IdentTopic,
    pub vote_topic: IdentTopic,

    pub round_timeout: Duration,
    pub round_start_time: Option<Instant>,
    pub is_leader: bool,

    pub prevotes: HashSet<PeerId>,
    pub precommits: HashSet<PeerId>,
    pub current_block_hash: Option<Vec<u8>>,
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
            leader_topic: IdentTopic::new("leader"),
            block_topic: IdentTopic::new("block-proposals"),
            vote_topic: IdentTopic::new("votes"),
            round_timeout: Duration::from_secs(10),
            round_start_time: None,
            is_leader: false,
            prevotes: HashSet::new(),
            precommits: HashSet::new(),
            current_block_hash: None,
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
