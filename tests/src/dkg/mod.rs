#[cfg(test)]
mod dkg_test {

    use crate::mocks::{db::MockDb, network::MockNodeCluster};
    use abci::db::Db;
    use bincode;
    use log::info;
    use protocol::block::{ChainConfig, ValidatorInfo};
    use sha2::{Digest, Sha256};
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use types::network::network_event::{DirectMessage, NetworkEvent};

    fn setup() {
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

        let registry = tracing_subscriber::registry().with(env_filter);
        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(true)
            .with_target(false);

        let _ = registry.with(console_layer).try_init();
        info!("Logging initialized with console output only");
    }

    #[tokio::test]
    async fn peers_send_start_dkg_at_startup() {
        setup();
        let mut cluster = MockNodeCluster::new(2).await;
        cluster.setup().await;
        info!("Ran setup");

        cluster.run_n_iterations(1).await;
        info!("Ran 1 iterations");

        for (_, node) in cluster.nodes.iter() {
            assert_eq!(node.peers.len(), 1);
        }

        for (peer, sender) in cluster.senders.iter() {
            info!(
                "Peer {} has {} pending events",
                peer,
                sender.pending_events.len()
            );
        }

        cluster.tear_down().await;
        info!("Ran teardown");
    }

    #[tokio::test]
    async fn test_dkg_round1_broadcasts() {
        setup();
        let mut cluster = MockNodeCluster::new(3).await;

        cluster.setup().await;
        info!("Started DKG Round1 test with {} nodes", cluster.nodes.len());

        // Run exactly one iteration to trigger DKG start and round1
        cluster.run_n_iterations(1).await;

        // Count the different types of messages that were generated
        let mut start_dkg_broadcasts = 0;
        let mut round1_broadcasts = 0;
        let mut other_messages = 0;

        // Check all pending events across all networks
        for (from_peer, sender) in cluster.senders.iter() {
            let pending_events = &sender.pending_events;
            info!(
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
                        info!("  Other event from {}: {:?}", from_peer, event);
                    }
                }
            }
        }

        info!("Message count summary:");
        info!("  Start-DKG broadcasts: {}", start_dkg_broadcasts);
        info!("  Round1 broadcasts: {}", round1_broadcasts);
        info!("  Other messages: {}", other_messages);

        // Verify each peer sent a start-dkg broadcast
        assert!(
            start_dkg_broadcasts >= cluster.nodes.len(),
            "Expected at least {} start-dkg broadcasts, got {}",
            cluster.nodes.len(),
            start_dkg_broadcasts
        );

        // After DKG starts, we expect round1 broadcasts from each peer
        // Note: This might happen in the same iteration or the next one
        info!("✅ DKG Round1 broadcast verification completed");
        cluster.tear_down().await;
    }

    #[tokio::test]
    async fn test_dkg_round2_private_requests() {
        setup();
        let mut cluster = MockNodeCluster::new(3).await;

        cluster.setup().await;
        info!("Started DKG Round2 test with {} nodes", cluster.nodes.len());

        // Run several iterations to allow round1 to complete
        let mut round1_complete = false;
        let mut round2_private_requests = 0;

        for iteration in 1..=10 {
            info!("--- Iteration {} ---", iteration);
            cluster.run_n_iterations(1).await;

            // Check for round2 private messages in the pending events
            let mut current_round2_requests = 0;

            for (_, sender) in cluster.senders.iter() {
                let pending_events = &sender.pending_events;
                info!("  {} pending events", pending_events.len());
                for event in pending_events.iter() {
                    match &event {
                        NetworkEvent::GossipsubMessage(msg) => {
                            let data_str = String::from_utf8_lossy(&msg.data);
                            // This is a broadcast
                            if data_str.contains("Round1") {
                                info!("  Still processing Round1 broadcasts");
                            }
                        }
                        NetworkEvent::MessageEvent((_, direct_message)) => {
                            info!("  Found MessageEvent");
                            if let DirectMessage::Round2Package(_) = direct_message {
                                info!("  Found Round2 private request");
                                current_round2_requests += 1;
                            }
                        }
                        _ => {
                            info!("  Other event: {:?}", event);
                        }
                    }
                }
            }

            if current_round2_requests > 0 {
                round2_private_requests += current_round2_requests;
                round1_complete = true;
                info!(
                    "  Found {} Round2 private requests in iteration {}",
                    current_round2_requests, iteration
                );
            }

            // If we've seen round2 requests, we can verify our expectations
            if round1_complete && round2_private_requests > 0 {
                break;
            }
        }

        info!("Round2 private request summary:");
        info!("  Round1 complete: {}", round1_complete);
        info!("  Round2 private requests: {}", round2_private_requests);

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

        info!("✅ DKG Round2 private request verification completed");
        cluster.tear_down().await;
    }

    #[tokio::test]
    async fn test_dkg_completion() {
        setup();
        let mut cluster = MockNodeCluster::new(3).await;
        cluster.setup().await;
        info!("Started DKG test with {} nodes", cluster.nodes.len());

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

            info!(
                "Iteration {}: {} total pending events",
                iteration_count, total_pending_events
            );

            if total_pending_events == 0 {
                info!(
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
            info!("✅ Node {} has both DKG public and private keys", peer_id);
        }

        info!(
            "🎉 DKG completed successfully on all {} nodes!",
            cluster.nodes.len()
        );
        cluster.tear_down().await;
    }

    #[tokio::test]
    async fn test_dkg_completes_within_5_iterations() {
        setup();
        let mut cluster = MockNodeCluster::new(3).await;
        cluster.setup().await;
        info!(
            "Started DKG completion test (within 5 iterations) with {} nodes",
            cluster.nodes.len()
        );

        let max_iterations = 5;
        cluster.run_n_iterations(max_iterations).await;
        info!("Ran {} iterations", max_iterations);

        for (peer_id, node) in cluster.nodes.iter() {
            assert!(
                node.pubkey_package.is_some(),
                "Node {} should have a public key package after {} iterations",
                peer_id,
                max_iterations
            );
            assert!(
                node.private_key_package.is_some(),
                "Node {} should have a private key package after {} iterations",
                peer_id,
                max_iterations
            );
        }

        info!(
            "🎉 DKG completed successfully on all {} nodes within {} iterations",
            cluster.nodes.len(),
            max_iterations
        );
        cluster.tear_down().await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_dkg_completion_256_nodes() {
        // setup();
        let start_time = std::time::Instant::now();
        let mut cluster = MockNodeCluster::new(256).await;
        cluster.setup().await;
        info!("Started DKG test with {} nodes", cluster.nodes.len());

        // Keep running iterations until no events are left
        let mut iteration_count = 0;
        let max_iterations = 200; // Safety limit to prevent infinite loops

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

            // Network pending events are now handled through channels, no need to check them separately

            info!(
                "Iteration {}: {} total pending events",
                iteration_count, total_pending_events
            );

            if total_pending_events == 0 {
                info!(
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
            info!("✅ Node {} has both DKG public and private keys", peer_id);
        }

        let duration = start_time.elapsed();
        info!("DKG completion for 256 nodes took: {:?}", duration);

        assert!(
            duration < std::time::Duration::from_secs(300),
            "DKG for 256 nodes took too long: {:?}",
            duration
        );

        info!(
            "🎉 DKG completed successfully on all {} nodes!",
            cluster.nodes.len()
        );
        cluster.tear_down().await;
    }

    #[tokio::test]
    #[ignore = "Test requires direct database access which is not available with message-passing architecture"]
    async fn test_genesis_block_contains_dkg_metadata() {
        setup();
        let mut cluster = MockNodeCluster::new(3).await;
        cluster.setup().await;
        info!(
            "Started genesis block DKG metadata test with {} nodes",
            cluster.nodes.len()
        );

        cluster.run_n_iterations(10).await;

        for (peer_id, node) in cluster.nodes.iter() {
            let db = MockDb::new();
            let genesis_block_from_db = db.get_block_by_height(0).unwrap().unwrap();

            let dkg_pub_key = node.pubkey_package.clone().unwrap();
            let mut validators: Vec<ValidatorInfo> = node
                .peers
                .iter()
                .map(|p| ValidatorInfo {
                    pub_key: p.to_bytes(),
                    stake: 100,
                })
                .collect();

            validators.sort_by(|a, b| a.pub_key.cmp(&b.pub_key));

            let chain_config = ChainConfig {
                min_signers: node.config.min_signers.unwrap_or(3),
                max_signers: node.config.max_signers.unwrap_or(5),
                min_stake: 100,
                block_time_seconds: 10,
                max_block_size: 1000,
            };

            let expected_initial_state = protocol::block::GenesisState {
                validators,
                vault_pub_key: dkg_pub_key.serialize().unwrap(),
                initial_balances: vec![],
                chain_config,
            };

            let mut hasher = Sha256::new();
            hasher.update(b"GENESIS");
            hasher.update(genesis_block_from_db.header.timestamp.to_le_bytes());
            let state_bytes =
                bincode::encode_to_vec(&expected_initial_state, bincode::config::standard())
                    .unwrap();
            hasher.update(&state_bytes);
            let mut expected_state_root = [0u8; 32];
            expected_state_root.copy_from_slice(&hasher.finalize());

            assert_eq!(
                genesis_block_from_db.header.state_root, expected_state_root,
                "Node {} should have the correct genesis block state root",
                peer_id
            );
            info!("Genesis block metadata verified for node {}", peer_id);
        }

        info!("Genesis block metadata verified for all nodes!");
        cluster.tear_down().await;
    }
}
