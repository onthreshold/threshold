#[cfg(test)]
pub mod signing_tests {
    use crate::mocks::network::MockNodeCluster;
    use node::swarm_manager::{DirectMessage, NetworkEvent, SelfRequest};
    use rand::RngCore;

    #[tokio::test]
    async fn signing_flow_completes_and_produces_shares() {
        let peers = 3;
        let min_signers = 2;
        let max_signers = 3;

        // Build cluster with pre-generated FROST keys – no DKG needed
        let mut cluster = MockNodeCluster::new_with_keys(peers, min_signers, max_signers).await;
        cluster.setup().await;

        // ── start signing ──
        let initiator = *cluster.nodes.keys().next().unwrap();
        let mut msg = [0u8; 32];
        rand::rng().fill_bytes(&mut msg);
        let hex_msg = hex::encode(msg);

        cluster.send_self_request_to_peer(
            initiator,
            SelfRequest::StartSigningSession {
                hex_message: hex_msg,
            },
        );

        // ── drive the network and count messages ──
        let (mut req, mut comm, mut pack, mut share) = (0, 0, 0, 0);
        for _ in 0..200 {
            cluster.run_n_iterations(1).await;

            // count DirectMessage traffic still queued in the mock senders
            for sender in cluster.senders.values() {
                for ev in &sender.pending_events {
                    if let NetworkEvent::MessageEvent((_, dm)) = ev {
                        match dm {
                            DirectMessage::SignRequest { .. } => req += 1,
                            DirectMessage::Commitments { .. } => comm += 1,
                            DirectMessage::SignPackage { .. } => pack += 1,
                            DirectMessage::SignatureShare { .. } => share += 1,
                            _ => {}
                        }
                    }
                }
            }
            // break once no messages are left in either channel
            if cluster
                .senders
                .values()
                .all(|s| s.pending_events.is_empty())
                && cluster
                    .networks
                    .values()
                    .all(|n| n.pending_events.lock().unwrap().is_empty())
            {
                break;
            }
        }

        // ── explicit assertions ──
        assert!(req >= min_signers as usize - 1, "missing SignRequest(s)");
        assert!(comm >= min_signers as usize - 1, "missing Commitments");
        assert!(pack >= min_signers as usize - 1, "missing SignPackage");
        assert!(share >= min_signers as usize - 1, "missing SignatureShare");

        // protocol should be finished – no pending spends
        for node in cluster.nodes.values() {
            let signing_state = node
                .handlers
                .iter()
                .find_map(|h| h.downcast_ref::<node::signing::SigningState>());
            assert!(
                signing_state.unwrap().pending_spends.is_empty(),
                "node still has pending spend"
            );
        }
    }
}
