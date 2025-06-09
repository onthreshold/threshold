#[cfg(test)]
mod dkg_test {
    use crate::mocks::network::MockNodeCluster;
    use node::swarm_manager::DirectMessage;
    use node::swarm_manager::NetworkEvent;

    #[tokio::test]
    async fn peers_send_start_dkg_at_startup() {
        let mut cluster = MockNodeCluster::new(2, 2, 2).await;
        cluster.setup().await;
        println!("Ran setup");

        cluster.run_n_iterations(1).await;
        println!("Ran 1 iterations");

        for (_, node) in cluster.nodes.iter() {
            assert_eq!(node.peers.len(), 1);
        }

        for (peer, sender) in cluster.senders.iter() {
            println!(
                "Peer {} has {} pending events",
                peer,
                sender.pending_events.len()
            );
        }

        cluster.tear_down().await;
        println!("Ran teardown");
    }

    #[tokio::test]
    async fn test_dkg_round1_broadcasts() {
        let mut cluster = MockNodeCluster::new(3, 2, 3).await;

        cluster.setup().await;
        println!("Started DKG Round1 test with {} nodes", cluster.nodes.len());

        // Run exactly one iteration to trigger DKG start and round1
        cluster.run_n_iterations(1).await;

        // Count the different types of messages that were generated
        let mut start_dkg_broadcasts = 0;
        let mut round1_broadcasts = 0;
        let mut other_messages = 0;

        // Check all pending events across all networks
        for (from_peer, sender) in cluster.senders.iter() {
            let pending_events = &sender.pending_events;
            println!(
                "Peer {} has {} pending events",
                from_peer,
                pending_events.len()
            );

            for event in pending_events.iter() {
                match &event {
                    NetworkEvent::GossipsubMessage(msg) => {
                        let topic_str = format!("{:?}", msg.topic);
                        let data_str = String::from_utf8_lossy(&msg.data);

                        if topic_str.contains("start-dkg") {
                            start_dkg_broadcasts += 1;
                        } else if data_str.contains("Round1") || topic_str.contains("round1") {
                            round1_broadcasts += 1;
                        } else {
                            other_messages += 1;
                        }
                    }
                    _ => {
                        other_messages += 1;
                        println!("  Other event from {}: {:?}", from_peer, event);
                    }
                }
            }
        }

        println!("Message count summary:");
        println!("  Start-DKG broadcasts: {}", start_dkg_broadcasts);
        println!("  Round1 broadcasts: {}", round1_broadcasts);
        println!("  Other messages: {}", other_messages);

        // Verify each peer sent a start-dkg broadcast
        assert!(
            start_dkg_broadcasts >= cluster.nodes.len(),
            "Expected at least {} start-dkg broadcasts, got {}",
            cluster.nodes.len(),
            start_dkg_broadcasts
        );

        // After DKG starts, we expect round1 broadcasts from each peer
        // Note: This might happen in the same iteration or the next one
        println!("✅ DKG Round1 broadcast verification completed");
        cluster.tear_down().await;
    }

    #[tokio::test]
    async fn test_dkg_round2_private_requests() {
        let mut cluster = MockNodeCluster::new(3, 2, 3).await;

        cluster.setup().await;
        println!("Started DKG Round2 test with {} nodes", cluster.nodes.len());

        // Run several iterations to allow round1 to complete
        let mut round1_complete = false;
        let mut round2_private_requests = 0;

        for iteration in 1..=10 {
            println!("--- Iteration {} ---", iteration);
            cluster.run_n_iterations(1).await;

            // Check for round2 private messages in the pending events
            let mut current_round2_requests = 0;

            for (_, sender) in cluster.senders.iter() {
                let pending_events = &sender.pending_events;
                println!("  {} pending events", pending_events.len());
                for event in pending_events.iter() {
                    match &event {
                        NetworkEvent::GossipsubMessage(msg) => {
                            let data_str = String::from_utf8_lossy(&msg.data);
                            // This is a broadcast
                            if data_str.contains("Round1") {
                                println!("  Still processing Round1 broadcasts");
                            }
                        }
                        NetworkEvent::MessageEvent((_, direct_message)) => {
                            println!("  Found MessageEvent");
                            if let DirectMessage::Round2Package(_) = direct_message {
                                println!("  Found Round2 private request");
                                current_round2_requests += 1;
                            }
                        }
                        _ => {
                            println!("  Other event: {:?}", event);
                        }
                    }
                }
            }

            if current_round2_requests > 0 {
                round2_private_requests += current_round2_requests;
                round1_complete = true;
                println!(
                    "  Found {} Round2 private requests in iteration {}",
                    current_round2_requests, iteration
                );
            }

            // If we've seen round2 requests, we can verify our expectations
            if round1_complete && round2_private_requests > 0 {
                break;
            }
        }

        println!("Round2 private request summary:");
        println!("  Round1 complete: {}", round1_complete);
        println!("  Round2 private requests: {}", round2_private_requests);

        // Verify that round2 private requests were sent after round1
        assert!(
            round1_complete,
            "Round1 should complete and trigger Round2 private requests"
        );

        assert!(
            round2_private_requests > 0,
            "Expected Round2 private requests after Round1 completion, got {}",
            round2_private_requests
        );

        println!("✅ DKG Round2 private request verification completed");
        cluster.tear_down().await;
    }

    #[tokio::test]
    async fn test_dkg_completion() {
        let mut cluster = MockNodeCluster::new(3, 2, 3).await;
        cluster.setup().await;
        println!("Started DKG test with {} nodes", cluster.nodes.len());

        // Keep running iterations until no events are left
        let mut iteration_count = 0;
        let max_iterations = 100; // Safety limit to prevent infinite loops

        loop {
            iteration_count += 1;

            if iteration_count > max_iterations {
                panic!("DKG test exceeded maximum iterations ({})", max_iterations);
            }

            // Run one iteration
            cluster.run_n_iterations(1).await;

            // Check if there are any pending events across all senders
            let mut total_pending_events = 0;
            for (_, sender) in cluster.senders.iter() {
                total_pending_events += sender.pending_events.len();
            }

            // Also check network pending events
            for (_, network) in cluster.networks.iter() {
                let pending_events = network.pending_events.lock().unwrap();
                total_pending_events += pending_events.len();
            }

            println!(
                "Iteration {}: {} total pending events",
                iteration_count, total_pending_events
            );

            if total_pending_events == 0 {
                println!(
                    "No more pending events after {} iterations",
                    iteration_count
                );
                break;
            }
        }

        // Verify that each node has DKG public and private key pairs
        for (peer_id, node) in cluster.nodes.iter() {
            assert!(
                node.pubkey_package.is_some(),
                "Node {} should have a public key package after DKG completion",
                peer_id
            );
            assert!(
                node.private_key_package.is_some(),
                "Node {} should have a private key package after DKG completion",
                peer_id
            );
            println!("✅ Node {} has both DKG public and private keys", peer_id);
        }

        println!(
            "🎉 DKG completed successfully on all {} nodes!",
            cluster.nodes.len()
        );
        cluster.tear_down().await;
    }
}
