use crate::{ConsensusPhase, ConsensusState};
use libp2p::PeerId;
use std::collections::HashSet;
use std::time::Duration;

#[test]
fn test_consensus_state_new() {
    let state = ConsensusState::new();

    assert_eq!(state.current_state, ConsensusPhase::WaitingForPropose);
    assert_eq!(state.current_round, 0);
    assert_eq!(state.current_height, 0);
    assert!(state.proposer.is_none());
    assert!(state.validators.is_empty());
    assert_eq!(state.round_timeout, Duration::from_secs(10));
    assert!(state.round_start_time.is_none());
    assert!(!state.is_leader);
    assert!(state.prevotes.is_empty());
    assert!(state.precommits.is_empty());
    assert!(state.current_block_hash.is_none());
    assert!(!state.block_finalized);
}

#[test]
fn test_consensus_state_default() {
    let state = ConsensusState::default();
    let new_state = ConsensusState::new();

    assert_eq!(state.current_state, new_state.current_state);
    assert_eq!(state.current_round, new_state.current_round);
    assert_eq!(state.current_height, new_state.current_height);
}

#[test]
fn test_select_leader_empty_validators() {
    let state = ConsensusState::new();

    let leader = state.select_leader(1);
    assert!(leader.is_none());
}

#[test]
fn test_select_leader_single_validator() {
    let mut state = ConsensusState::new();
    let peer_id = PeerId::random();
    state.validators.insert(peer_id);

    let leader = state.select_leader(1);
    assert_eq!(leader, Some(peer_id));

    // Same leader for any round with single validator
    let leader2 = state.select_leader(5);
    assert_eq!(leader2, Some(peer_id));
}

#[test]
fn test_select_leader_multiple_validators() {
    let mut state = ConsensusState::new();

    // Create deterministic peer IDs for consistent testing
    let mut peer_ids: Vec<PeerId> = (0..3).map(|_| PeerId::random()).collect();
    peer_ids.sort(); // Sort to ensure deterministic order

    for &peer_id in &peer_ids {
        state.validators.insert(peer_id);
    }

    // Test leader selection for different rounds
    let leader1 = state.select_leader(1);
    let leader2 = state.select_leader(2);
    let leader3 = state.select_leader(3);
    let leader4 = state.select_leader(4); // Should wrap around

    assert!(leader1.is_some());
    assert!(leader2.is_some());
    assert!(leader3.is_some());
    assert!(leader4.is_some());

    // Leaders should be from validator set
    assert!(state.validators.contains(&leader1.unwrap()));
    assert!(state.validators.contains(&leader2.unwrap()));
    assert!(state.validators.contains(&leader3.unwrap()));
    assert!(state.validators.contains(&leader4.unwrap()));

    // Round 4 should be same as round 1 (round % validator_count)
    assert_eq!(leader1, leader4);
}

#[test]
fn test_select_leader_round_robin() {
    let mut state = ConsensusState::new();

    // Add exactly 3 validators
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();
    let peer3 = PeerId::random();

    state.validators.insert(peer1);
    state.validators.insert(peer2);
    state.validators.insert(peer3);

    // Test that leaders cycle through validators
    let mut selected_leaders = HashSet::new();
    for round in 1..=6 {
        // Test 2 full cycles
        let leader = state.select_leader(round);
        assert!(leader.is_some());
        selected_leaders.insert(leader.unwrap());
    }

    // All validators should have been selected as leaders
    assert_eq!(selected_leaders.len(), 3);
    assert!(selected_leaders.contains(&peer1));
    assert!(selected_leaders.contains(&peer2));
    assert!(selected_leaders.contains(&peer3));
}

#[test]
fn test_validator_management() {
    let mut state = ConsensusState::new();

    assert!(state.validators.is_empty());

    let peer1 = PeerId::random();
    let peer2 = PeerId::random();

    // Add validators
    state.validators.insert(peer1);
    assert_eq!(state.validators.len(), 1);
    assert!(state.validators.contains(&peer1));

    state.validators.insert(peer2);
    assert_eq!(state.validators.len(), 2);
    assert!(state.validators.contains(&peer1));
    assert!(state.validators.contains(&peer2));

    // Adding same validator again should not increase count
    state.validators.insert(peer1);
    assert_eq!(state.validators.len(), 2);
}

#[test]
fn test_vote_tracking() {
    let mut state = ConsensusState::new();

    let peer1 = PeerId::random();
    let peer2 = PeerId::random();

    // Initially empty
    assert!(state.prevotes.is_empty());
    assert!(state.precommits.is_empty());

    // Add prevotes
    state.prevotes.insert(peer1);
    assert_eq!(state.prevotes.len(), 1);
    assert!(state.prevotes.contains(&peer1));

    state.prevotes.insert(peer2);
    assert_eq!(state.prevotes.len(), 2);

    // Add precommits
    state.precommits.insert(peer1);
    assert_eq!(state.precommits.len(), 1);
    assert!(state.precommits.contains(&peer1));

    // Adding same peer again should not increase count
    state.prevotes.insert(peer1);
    assert_eq!(state.prevotes.len(), 2);

    state.precommits.insert(peer1);
    assert_eq!(state.precommits.len(), 1);
}

#[test]
fn test_round_state_management() {
    let mut state = ConsensusState::new();

    // Initial state
    assert_eq!(state.current_round, 0);
    assert_eq!(state.current_state, ConsensusPhase::WaitingForPropose);
    assert!(!state.is_leader);
    assert!(!state.block_finalized);

    // Simulate round progression
    state.current_round = 1;
    state.current_state = ConsensusPhase::Propose;
    state.is_leader = true;
    state.block_finalized = true;

    assert_eq!(state.current_round, 1);
    assert_eq!(state.current_state, ConsensusPhase::Propose);
    assert!(state.is_leader);
    assert!(state.block_finalized);
}
