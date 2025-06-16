#[cfg(test)]
mod consensus_tests {
    use std::collections::HashSet;

    use crate::mocks::network::MockNodeCluster;
    use libp2p::PeerId;
    use node::handlers::consensus::ConsensusState;
    use types::network_event::NetworkEvent;

    #[tokio::test]
    async fn leader_remains_consistent_across_nodes() {
        let mut cluster = MockNodeCluster::new_with_keys(4, 2, 4).await;
        cluster.setup().await;

        let leader_topic = libp2p::gossipsub::IdentTopic::new("leader");
        let peer_ids = cluster.get_peer_ids();

        for (recipient_peer, network) in cluster.networks.iter() {
            for peer_id in peer_ids.iter().filter(|id| **id != *recipient_peer) {
                let _ = network.events_emitter_tx.send(NetworkEvent::Subscribed {
                    peer_id: *peer_id,
                    topic: leader_topic.hash(),
                });
            }
        }

        cluster.run_n_iterations(30).await;

        let peer_count = cluster.get_peer_ids().len();
        let mut attempts = 0;
        loop {
            let mut all_ready = true;
            for node in cluster.nodes.values() {
                let cs = node
                    .handlers
                    .iter()
                    .find_map(|h| h.downcast_ref::<ConsensusState>())
                    .expect("ConsensusState not found");
                if cs.validators.len() != peer_count {
                    all_ready = false;
                    break;
                }
            }

            if all_ready || attempts > 20 {
                break;
            }

            cluster.run_n_iterations(5).await;
            attempts += 1;
        }

        let mut leaders: HashSet<PeerId> = HashSet::new();
        for node in cluster.nodes.values() {
            let cs = node
                .handlers
                .iter()
                .find_map(|h| h.downcast_ref::<ConsensusState>())
                .expect("ConsensusState missing");

            assert_eq!(cs.validators.len(), peer_count, "Validator set incomplete");
            let leader = cs.select_leader(1).expect("No leader selected");
            leaders.insert(leader);
        }

        assert_eq!(
            leaders.len(),
            1,
            "Nodes disagree on selected leader: {:?}",
            leaders
        );
    }
}
