use crate::{
    ConsensusInterface, ConsensusInterfaceImpl, ConsensusMessage, ConsensusPhase, ConsensusResponse,
};
use libp2p::PeerId;
use tokio::sync::broadcast;
use types::consensus::{Vote, VoteType};

#[tokio::test]
async fn test_consensus_interface_creation() {
    let (interface, _tx) = ConsensusInterfaceImpl::new();

    assert_eq!(interface.state.current_round, 0);
    assert_eq!(interface.state.current_height, 0);
    assert_eq!(
        interface.state.current_state,
        ConsensusPhase::WaitingForPropose
    );
    assert!(interface.state.validators.is_empty());
    assert!(interface.peer_id.is_none());
    assert!(interface.network_events_tx.is_none());
    assert!(interface.chain_interface_tx.is_none());
}

#[tokio::test]
async fn test_add_validator() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let peer_id = PeerId::random();

    assert_eq!(interface.state.validators.len(), 0);

    let response = interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer_id.to_bytes(),
        })
        .await;

    match response {
        ConsensusResponse::AddValidator { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.validators.len(), 1);
    assert!(interface.state.validators.contains(&peer_id));
}

#[tokio::test]
async fn test_add_multiple_validators() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();

    // Add first validator
    let response1 = interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer1.to_bytes(),
        })
        .await;

    match response1 {
        ConsensusResponse::AddValidator { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    // Add second validator
    let response2 = interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer2.to_bytes(),
        })
        .await;

    match response2 {
        ConsensusResponse::AddValidator { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.validators.len(), 2);
    assert!(interface.state.validators.contains(&peer1));
    assert!(interface.state.validators.contains(&peer2));
}

#[tokio::test]
async fn test_add_validator_invalid_peer_id() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();

    let response = interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: vec![1, 2, 3], // Invalid peer ID bytes
        })
        .await;

    match response {
        ConsensusResponse::AddValidator { error } => {
            assert!(error.is_some());
            assert!(error.unwrap().contains("Failed to decode peer ID"));
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.validators.len(), 0);
}

#[tokio::test]
async fn test_set_leader() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();

    assert!(!interface.state.is_leader);

    let response = interface
        .handle_message(ConsensusMessage::SetLeader { is_leader: true })
        .await;

    match response {
        ConsensusResponse::SetLeader { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert!(interface.state.is_leader);

    // Set to false
    let response2 = interface
        .handle_message(ConsensusMessage::SetLeader { is_leader: false })
        .await;

    match response2 {
        ConsensusResponse::SetLeader { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert!(!interface.state.is_leader);
}

#[tokio::test]
async fn test_get_consensus_state() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let peer_id = PeerId::random();

    // Add a validator first
    interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer_id.to_bytes(),
        })
        .await;

    // Set as leader
    interface
        .handle_message(ConsensusMessage::SetLeader { is_leader: true })
        .await;

    let response = interface
        .handle_message(ConsensusMessage::GetConsensusState)
        .await;

    match response {
        ConsensusResponse::GetConsensusState {
            phase,
            round,
            height,
            is_leader,
            validators_count,
            prevotes_count,
            precommits_count,
        } => {
            assert_eq!(phase, ConsensusPhase::WaitingForPropose);
            assert_eq!(round, 0);
            assert_eq!(height, 0);
            assert!(is_leader);
            assert_eq!(validators_count, 1);
            assert_eq!(prevotes_count, 0);
            assert_eq!(precommits_count, 0);
        }
        _ => panic!("Unexpected response type"),
    }
}

#[tokio::test]
async fn test_trigger_consensus_round() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let peer_id = PeerId::random();

    // Add a validator first to enable round triggering
    interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer_id.to_bytes(),
        })
        .await;

    let initial_round = interface.state.current_round;

    let response = interface
        .handle_message(ConsensusMessage::TriggerConsensusRound { force_round: false })
        .await;

    match response {
        ConsensusResponse::TriggerConsensusRound {
            success,
            message: _,
            round_number,
        } => {
            assert!(success);
            assert_eq!(round_number, (initial_round + 1) as u64);
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.current_round, initial_round + 1);
}

#[tokio::test]
async fn test_handle_vote_from_validator() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let voter_peer = PeerId::random();
    let other_peer = PeerId::random();

    // Add validators
    interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: voter_peer.to_bytes(),
        })
        .await;
    interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: other_peer.to_bytes(),
        })
        .await;

    let vote = Vote {
        round: 1,
        height: 0,
        block_hash: vec![1, 2, 3, 4],
        voter: voter_peer.to_bytes(),
        vote_type: VoteType::Prevote,
    };

    let response = interface
        .handle_message(ConsensusMessage::HandleVote {
            sender: voter_peer.to_bytes(),
            vote,
        })
        .await;

    match response {
        ConsensusResponse::HandleVote { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }
}

#[tokio::test]
async fn test_handle_vote_invalid_sender() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();

    let vote = Vote {
        round: 1,
        height: 0,
        block_hash: vec![1, 2, 3, 4],
        voter: vec![1, 2, 3], // Invalid voter
        vote_type: VoteType::Prevote,
    };

    let response = interface
        .handle_message(ConsensusMessage::HandleVote {
            sender: vec![1, 2, 3], // Invalid sender
            vote,
        })
        .await;

    match response {
        ConsensusResponse::HandleVote { error } => {
            assert!(error.is_some());
            assert!(error.unwrap().contains("Failed to decode sender peer ID"));
        }
        _ => panic!("Unexpected response type"),
    }
}

#[tokio::test]
async fn test_handle_leader_announcement() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let sender_peer = PeerId::random();
    let leader_peer = PeerId::random();

    let response = interface
        .handle_message(ConsensusMessage::HandleLeaderAnnouncement {
            sender: sender_peer.to_bytes(),
            leader: leader_peer.to_bytes(),
            round: 1,
        })
        .await;

    match response {
        ConsensusResponse::HandleLeaderAnnouncement { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.current_round, 1);
    assert_eq!(interface.state.proposer, Some(leader_peer));
}

#[tokio::test]
async fn test_handle_new_round() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let sender_peer = PeerId::random();

    let initial_round = interface.state.current_round;

    let response = interface
        .handle_message(ConsensusMessage::HandleNewRound {
            sender: sender_peer.to_bytes(),
            round: 3,
        })
        .await;

    match response {
        ConsensusResponse::HandleNewRound { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    // Should advance to round 3-1=2, then start_new_round increments to 3
    assert!(interface.state.current_round > initial_round);
}

#[tokio::test]
async fn test_network_events_setup() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let (network_tx, _network_rx) = broadcast::channel(100);

    assert!(interface.network_events_tx.is_none());

    interface.set_network_events_tx(network_tx.clone());

    assert!(interface.network_events_tx.is_some());
}

#[tokio::test]
async fn test_peer_id_setup() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let peer_id = PeerId::random();

    assert!(interface.peer_id.is_none());

    interface.set_peer_id(peer_id);

    assert_eq!(interface.peer_id, Some(peer_id));
}

#[tokio::test]
async fn test_auto_start_consensus_when_max_validators_reached() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();

    // Set max validators to 2
    interface.set_max_validators(2);

    let peer1 = PeerId::random();
    let peer2 = PeerId::random();

    // Add first validator - should not trigger consensus
    let response1 = interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer1.to_bytes(),
        })
        .await;

    match response1 {
        ConsensusResponse::AddValidator { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.current_round, 0); // Still no consensus

    // Add second validator - should trigger consensus
    let response2 = interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer2.to_bytes(),
        })
        .await;

    match response2 {
        ConsensusResponse::AddValidator { error } => {
            assert!(error.is_none());
        }
        _ => panic!("Unexpected response type"),
    }

    assert_eq!(interface.state.current_round, 1); // Consensus should have started
    assert_eq!(interface.state.validators.len(), 2);
}

#[tokio::test]
async fn test_no_auto_start_without_max_validators_set() {
    let (mut interface, _tx) = ConsensusInterfaceImpl::new();
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();

    // Don't set max validators

    // Add multiple validators
    interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer1.to_bytes(),
        })
        .await;

    interface
        .handle_message(ConsensusMessage::AddValidator {
            peer_id: peer2.to_bytes(),
        })
        .await;

    // Should not auto-start consensus
    assert_eq!(interface.state.current_round, 0);
    assert_eq!(interface.state.validators.len(), 2);
}
