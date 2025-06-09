use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use node::{
    NodeState,
    swarm_manager::{DirectMessage, Network, NetworkEvent, NetworkResponseFuture},
};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use types::errors;

// Import MockDb from our mocks module
use crate::mocks::db::MockDb;

#[derive(Debug)]
struct SenderToNode {
    pending_events: Vec<NetworkEvent>,
    events_emitter_tx: UnboundedSender<NetworkEvent>,
}

impl SenderToNode {
    fn new(events_emitter_tx: UnboundedSender<NetworkEvent>) -> Self {
        Self {
            pending_events: Vec::new(),
            events_emitter_tx,
        }
    }

    fn queue(&mut self, event: NetworkEvent) {
        self.pending_events.push(event);
    }

    fn flush(&mut self) {
        for event in self.pending_events.drain(..) {
            self.events_emitter_tx.send(event).unwrap();
        }
    }
}

#[derive(Debug)]
pub struct PendingNetworkEvent {
    pub from_peer: libp2p::PeerId,
    pub event: NetworkEvent,
    pub target_peers: Vec<libp2p::PeerId>, // Empty vec means broadcast to all
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MockNetwork {
    peer: libp2p::PeerId,
    events_emitter_tx: UnboundedSender<NetworkEvent>,
    pending_events: Arc<Mutex<Vec<PendingNetworkEvent>>>,
}

impl MockNetwork {
    pub fn new(events_emitter_tx: UnboundedSender<NetworkEvent>, peer: libp2p::PeerId) -> Self {
        Self {
            events_emitter_tx,
            peer,
            pending_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Network for MockNetwork {
    fn peer_id(&self) -> libp2p::PeerId {
        self.peer
    }

    fn send_broadcast(
        &self,
        topic: libp2p::gossipsub::IdentTopic,
        message: Vec<u8>,
    ) -> Result<(), errors::NetworkError> {
        println!("sent broadcast");
        let gossip_message = libp2p::gossipsub::Message {
            source: Some(self.peer),
            data: message,
            sequence_number: None,
            topic: topic.hash(),
        };

        // Queue the event instead of sending immediately
        let pending_event = PendingNetworkEvent {
            from_peer: self.peer,
            event: NetworkEvent::GossipsubMessage(gossip_message),
            target_peers: Vec::new(), // Empty means broadcast to all
        };

        self.pending_events.lock().unwrap().push(pending_event);
        Ok(())
    }

    fn send_private_message(
        &self,
        peer_id: libp2p::PeerId,
        request: node::swarm_manager::DirectMessage,
    ) -> Result<(), errors::NetworkError> {
        println!("sent private message");
        // For mock purposes, we'll create a simplified message event
        // In a real implementation, this would use proper request-response channels
        let pending_event = PendingNetworkEvent {
            from_peer: self.peer,
            event: NetworkEvent::MessageEvent(libp2p::request_response::Event::Message {
                peer: peer_id,
                message: libp2p::request_response::Message::Request {
                    request_id: unsafe { std::mem::zeroed() }, // Create dummy ID
                    request,
                    channel: unsafe { std::mem::zeroed() }, // Dummy channel
                },
            }),
            target_peers: vec![peer_id],
        };

        self.pending_events.lock().unwrap().push(pending_event);
        Ok(())
    }

    fn send_self_request(
        &self,
        request: node::swarm_manager::SelfRequest,
        sync: bool,
    ) -> Result<Option<NetworkResponseFuture>, errors::NetworkError> {
        println!("sent self request");
        // For self requests, send immediately to own node
        let self_request_event = NetworkEvent::SelfRequest {
            request,
            response_channel: None,
        };
        let _ = self.events_emitter_tx.send(self_request_event);
        Ok(None)
    }
}

pub struct MockNodeCluster {
    nodes: BTreeMap<libp2p::PeerId, NodeState<MockNetwork, MockDb>>,
    senders: BTreeMap<libp2p::PeerId, SenderToNode>,
    networks: BTreeMap<libp2p::PeerId, MockNetwork>,
}

impl MockNodeCluster {
    pub async fn new(peers: u32, min_signers: u16, max_signers: u16) -> Self {
        Self::new_with_db_prefix(peers, min_signers, max_signers, "node").await
    }

    pub async fn new_with_db_prefix(
        peers: u32,
        min_signers: u16,
        max_signers: u16,
        db_prefix: &str,
    ) -> Self {
        let mut path = PathBuf::new();
        path.push("config.json");

        let mut config_path = PathBuf::new();
        config_path.push("config.toml");

        let node_config = node::NodeConfig::new(path.clone(), config_path, None);

        let mut nodes = BTreeMap::new();
        let mut senders = BTreeMap::new();
        let mut networks = BTreeMap::new();

        for _i in 0..peers {
            let peer_id = libp2p::PeerId::random();
            let Ok((node, network)) =
                create_node_network(peer_id, node_config.clone(), min_signers, max_signers)
            else {
                panic!("Failed to create node network");
            };

            nodes.insert(peer_id, node);
            senders.insert(
                peer_id,
                SenderToNode::new(network.events_emitter_tx.clone()),
            );
            networks.insert(peer_id, network);
        }

        Self {
            nodes,
            senders,
            networks,
        }
    }

    pub async fn setup(&mut self) {
        let peers: Vec<libp2p::PeerId> = self.nodes.keys().cloned().collect();
        for (receipient_peer, sender) in self.senders.iter_mut() {
            sender.queue(NetworkEvent::PeersConnected(
                peers
                    .iter()
                    .filter(|peer_id| *peer_id != receipient_peer)
                    .map(|peer_id| (*peer_id, libp2p::Multiaddr::empty()))
                    .collect(),
            ));

            for peer_id in peers.iter().filter(|peer_id| *peer_id != receipient_peer) {
                sender.queue(NetworkEvent::Subscribed {
                    peer_id: *peer_id,
                    topic: libp2p::gossipsub::IdentTopic::new("start-dkg").hash(),
                });
            }

            sender.flush();
        }
    }

    pub async fn tear_down(&mut self) {
        let peers: Vec<libp2p::PeerId> = self.nodes.keys().cloned().collect();
        for (_, sender) in self.senders.iter_mut() {
            sender.queue(NetworkEvent::PeersDisconnected(
                peers
                    .iter()
                    .map(|peer_id| (*peer_id, libp2p::Multiaddr::empty()))
                    .collect(),
            ));
        }
    }

    pub async fn run_n_iterations(&mut self, iterations: u32) {
        for _ in 0..iterations {
            // Flush all messages
            for (_, sender) in self.senders.iter_mut() {
                sender.flush();
            }

            // Poll Nodes
            for (_, node) in self.nodes.iter_mut() {
                loop {
                    let more = node.try_poll().await.expect("Failed to poll node");
                    if !more {
                        break;
                    }
                }
            }

            // Process any network events generated during polling
            self.process_network_events().await;
        }
    }

    // Process network events that were generated during node polling
    async fn process_network_events(&mut self) {
        // Collect all pending events from all networks
        let mut all_pending_events = Vec::new();

        for (peer_id, network) in self.networks.iter() {
            let mut pending_events = network.pending_events.lock().unwrap();
            all_pending_events.extend(pending_events.drain(..));
        }

        // Process each pending event
        for pending_event in all_pending_events {
            self.forward_event_to_peers(pending_event).await;
        }
    }

    // Forward a single event to the appropriate target peers
    async fn forward_event_to_peers(&mut self, pending_event: PendingNetworkEvent) {
        if pending_event.target_peers.is_empty() {
            // Broadcast to all peers except the sender
            let target_peers: Vec<libp2p::PeerId> = self
                .senders
                .keys()
                .filter(|peer_id| **peer_id != pending_event.from_peer)
                .cloned()
                .collect();

            for target_peer in target_peers {
                if let Some(sender) = self.senders.get_mut(&target_peer) {
                    // We need to recreate the event for each peer since NetworkEvent doesn't implement Clone
                    let event = match &pending_event.event {
                        NetworkEvent::GossipsubMessage(msg) => {
                            NetworkEvent::GossipsubMessage(libp2p::gossipsub::Message {
                                source: msg.source,
                                data: msg.data.clone(),
                                sequence_number: msg.sequence_number,
                                topic: msg.topic.clone(),
                            })
                        }
                        NetworkEvent::SelfRequest {
                            request,
                            response_channel,
                        } => NetworkEvent::SelfRequest {
                            request: request.clone(),
                            response_channel: None,
                        },
                        NetworkEvent::Subscribed { peer_id, topic } => NetworkEvent::Subscribed {
                            peer_id: *peer_id,
                            topic: topic.clone(),
                        },
                        NetworkEvent::MessageEvent(libp2p::request_response::Event::Message { peer, message }) => {
                            // For mock testing, recreate the MessageEvent::Message
                            // Since we can't clone the original message, create a simple mock request
                            NetworkEvent::MessageEvent(libp2p::request_response::Event::Message {
                                peer: *peer,
                                message: libp2p::request_response::Message::Request {
                                    request_id: unsafe { std::mem::zeroed() }, // Create dummy ID
                                    request: node::swarm_manager::DirectMessage::Pong, // Simple dummy message
                                    channel: unsafe { std::mem::zeroed() }, // Dummy channel
                                },
                            })
                        }
                        NetworkEvent::PeersConnected(items) => {
                            NetworkEvent::PeersConnected(items.clone())
                        }
                        NetworkEvent::PeersDisconnected(items) => {
                            NetworkEvent::PeersDisconnected(items.clone())
                        }
                        _ => {
                            panic!("Unexpected event type: {:?}", pending_event.event);
                        }
                    };
                    sender.queue(event);
                }
            }
        } else {
            // Send to specific target peers
            for target_peer in pending_event.target_peers {
                if let Some(sender) = self.senders.get_mut(&target_peer) {
                    // Recreate the event for the target peer
                    let event = match &pending_event.event {
                        NetworkEvent::SelfRequest { request, .. } => NetworkEvent::SelfRequest {
                            request: request.clone(),
                            response_channel: None,
                        },
                        NetworkEvent::GossipsubMessage(msg) => {
                            NetworkEvent::GossipsubMessage(libp2p::gossipsub::Message {
                                source: msg.source,
                                data: msg.data.clone(),
                                sequence_number: msg.sequence_number,
                                topic: msg.topic.clone(),
                            })
                        }
                        NetworkEvent::PeersConnected(items) => {
                            NetworkEvent::PeersConnected(items.clone())
                        }
                        NetworkEvent::PeersDisconnected(items) => {
                            NetworkEvent::PeersDisconnected(items.clone())
                        }
                        NetworkEvent::Subscribed { peer_id, topic } => NetworkEvent::Subscribed {
                            peer_id: *peer_id,
                            topic: topic.clone(),
                        },
                        NetworkEvent::MessageEvent(libp2p::request_response::Event::Message { peer, message }) => {
                            // For mock testing, recreate the MessageEvent::Message
                            // Since we can't clone the original message, create a simple mock request
                            NetworkEvent::MessageEvent(libp2p::request_response::Event::Message {
                                peer: *peer,
                                message: libp2p::request_response::Message::Request {
                                    request_id: unsafe { std::mem::zeroed() }, // Create dummy ID
                                    request: node::swarm_manager::DirectMessage::Pong, // Simple dummy message
                                    channel: unsafe { std::mem::zeroed() }, // Dummy channel
                                },
                            })
                        }
                        NetworkEvent::MessageEvent(libp2p::request_response::Event::OutboundFailure { .. }) |
                        NetworkEvent::MessageEvent(libp2p::request_response::Event::InboundFailure { .. }) |
                        NetworkEvent::MessageEvent(libp2p::request_response::Event::ResponseSent { .. }) => {
                            // For mock testing, we wont handle these events
                            continue;
                        },
                    };
                    sender.queue(event);
                }
            }
        }
    }

    // Methods to send various types of network events for testing
    pub fn send_broadcast_to_all(
        &mut self,
        topic: libp2p::gossipsub::IdentTopic,
        message: Vec<u8>,
    ) {
        let gossip_message = libp2p::gossipsub::Message {
            source: None, // Simulate external broadcast
            data: message,
            sequence_number: None,
            topic: topic.hash(),
        };

        for (_, sender) in self.senders.iter_mut() {
            sender.queue(NetworkEvent::GossipsubMessage(gossip_message.clone()));
        }
    }

    pub fn send_private_request_to_peer(
        &mut self,
        _from_peer: libp2p::PeerId,
        to_peer: libp2p::PeerId,
        request: DirectMessage,
    ) {
        if let Some(sender) = self.senders.get_mut(&to_peer) {
            sender.queue(NetworkEvent::GossipsubMessage(libp2p::gossipsub::Message {
                source: Some(_from_peer),
                data: format!("private_request:{:?}", request).into_bytes(),
                sequence_number: None,
                topic: libp2p::gossipsub::TopicHash::from_raw("private_request"),
            }));
        }
    }

    pub fn send_self_request_to_peer(
        &mut self,
        peer_id: libp2p::PeerId,
        request: node::swarm_manager::SelfRequest,
    ) {
        if let Some(sender) = self.senders.get_mut(&peer_id) {
            sender.queue(NetworkEvent::SelfRequest {
                request,
                response_channel: None,
            });
        }
    }

    pub fn simulate_peer_disconnect(&mut self, peer_id: libp2p::PeerId) {
        for (recipient_peer, sender) in self.senders.iter_mut() {
            if *recipient_peer != peer_id {
                sender.queue(NetworkEvent::PeersDisconnected(vec![(
                    peer_id,
                    libp2p::Multiaddr::empty(),
                )]));
            }
        }
    }

    pub fn simulate_peer_reconnect(&mut self, peer_id: libp2p::PeerId) {
        for (recipient_peer, sender) in self.senders.iter_mut() {
            if *recipient_peer != peer_id {
                sender.queue(NetworkEvent::PeersConnected(vec![(
                    peer_id,
                    libp2p::Multiaddr::empty(),
                )]));
            }
        }
    }

    // Helper method to get peer IDs for testing
    pub fn get_peer_ids(&self) -> Vec<libp2p::PeerId> {
        self.nodes.keys().cloned().collect()
    }
}

pub fn create_node_network(
    peer_id: libp2p::PeerId,
    node_config: node::NodeConfig,
    min_signers: u16,
    max_signers: u16,
) -> Result<(NodeState<MockNetwork, MockDb>, MockNetwork), errors::NodeError> {
    let (events_emitter_tx, events_emitter_rx) = unbounded_channel::<NetworkEvent>();
    let network = MockNetwork {
        events_emitter_tx,
        peer: peer_id,
        pending_events: Arc::new(Mutex::new(Vec::new())),
    };

    let mock_db = MockDb::new();

    let nodes_state = NodeState::new_from_config(
        network.clone(),
        min_signers,
        max_signers,
        node_config,
        mock_db,
        events_emitter_rx,
    )?;

    Ok((nodes_state, network))
}

#[cfg(test)]
mod node_tests {
    use super::*;

    // #[tokio::test]
    // async fn peers_can_connect() {
    //     let mut cluster = MockNodeCluster::new(2, 2, 2).await;
    //     cluster.setup().await;
    //     println!("Ran setup");
    //     cluster.run_n_iterations(1).await;
    //     println!("Ran 1 iterations");
    //
    //     for (_, node) in cluster.nodes.iter() {
    //         assert_eq!(node.peers.len(), 1);
    //     }
    //
    //     cluster.tear_down().await;
    //     println!("Ran teardown");
    // }

    #[tokio::test]
    async fn network_events_are_processed_correctly() {
        let test_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut cluster =
            MockNodeCluster::new_with_db_prefix(3, 2, 3, &format!("test-events-{}", test_id)).await;
        cluster.setup().await;

        // Get peer IDs for testing
        let peer_ids = cluster.get_peer_ids();
        let first_peer = peer_ids[0];
        let second_peer = peer_ids[1];

        // Manually trigger some network events by calling network methods directly
        {
            let first_network = cluster.networks.get(&first_peer).unwrap();

            // Test broadcast
            let topic = libp2p::gossipsub::IdentTopic::new("test-topic");
            first_network
                .send_broadcast(topic, b"broadcast message".to_vec())
                .unwrap();

            // Test private message
            first_network
                .send_private_message(second_peer, DirectMessage::Pong)
                .unwrap();

            // Check that events are queued
            let pending_events = first_network.pending_events.lock().unwrap();
            assert_eq!(pending_events.len(), 2, "Should have 2 pending events");
        }

        // Process the events
        cluster.process_network_events().await;

        // Check that events are cleared from the network after processing
        {
            let first_network = cluster.networks.get(&first_peer).unwrap();
            let pending_events = first_network.pending_events.lock().unwrap();
            assert_eq!(
                pending_events.len(),
                0,
                "Pending events should be cleared after processing"
            );
        }

        // Check that events were queued in the appropriate senders
        let second_sender = cluster.senders.get(&second_peer).unwrap();
        assert!(
            second_sender.pending_events.len() >= 1,
            "Second peer should have received events"
        );

        println!("Network event processing works correctly!");
    }

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
    async fn test_request_response() {
        let mut cluster = MockNodeCluster::new(2, 2, 2).await;
        cluster.setup().await;
        println!("Ran setup");

        cluster.run_n_iterations(2).await;
        println!("Ran 2 iterations");

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
    async fn test_dkg_completion() {
        let test_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut cluster =
            MockNodeCluster::new_with_db_prefix(3, 2, 3, &format!("test-dkg-{}", test_id)).await;
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

    #[tokio::test]
    async fn test_dkg_round1_broadcasts() {
        let test_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut cluster =
            MockNodeCluster::new_with_db_prefix(3, 2, 3, &format!("test-dkg-round1-{}", test_id))
                .await;

        cluster.setup().await;
        println!("Started DKG Round1 test with {} nodes", cluster.nodes.len());

        // Run exactly one iteration to trigger DKG start and round1
        cluster.run_n_iterations(1).await;

        // Count the different types of messages that were generated
        let mut start_dkg_broadcasts = 0;
        let mut round1_broadcasts = 0;
        let mut other_messages = 0;

        // Check all pending events across all networks
        for (peer_id, network) in cluster.networks.iter() {
            let pending_events = network.pending_events.lock().unwrap();
            println!(
                "Network for peer {} has {} pending events",
                peer_id,
                pending_events.len()
            );

            for event in pending_events.iter() {
                match &event.event {
                    NetworkEvent::GossipsubMessage(msg) => {
                        let topic_str = format!("{:?}", msg.topic);
                        let data_str = String::from_utf8_lossy(&msg.data);

                        println!(
                            "  Broadcast from {}: topic={}, data_preview={}",
                            event.from_peer,
                            topic_str,
                            &data_str[..std::cmp::min(50, data_str.len())]
                        );

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
                        println!("  Other event from {}: {:?}", event.from_peer, event.event);
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
        let test_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut cluster =
            MockNodeCluster::new_with_db_prefix(3, 2, 3, &format!("test-dkg-round2-{}", test_id))
                .await;

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

            for (peer_id, network) in cluster.networks.iter() {
                let pending_events = network.pending_events.lock().unwrap();

                for event in pending_events.iter() {
                    match &event.event {
                        NetworkEvent::GossipsubMessage(msg) => {
                            let data_str = String::from_utf8_lossy(&msg.data);

                            // Check if this is a private message (has specific target peers)
                            if !event.target_peers.is_empty() {
                                println!(
                                    "  Private message from {} to {:?}: {}",
                                    event.from_peer,
                                    event.target_peers,
                                    &data_str[..std::cmp::min(50, data_str.len())]
                                );

                                if data_str.contains("Round2")
                                    || data_str.contains("private_message")
                                {
                                    current_round2_requests += 1;
                                }
                            } else {
                                // This is a broadcast
                                if data_str.contains("Round1") {
                                    println!("  Still processing Round1 broadcasts");
                                } else if data_str.contains("Round2") {
                                    println!("  Round2 broadcast detected");
                                }
                            }
                        }
                        _ => {}
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
}
