use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use frost_secp256k1::Identifier;
use node::{
    NodeState,
    swarm_manager::{Network, NetworkResponseFuture},
    wallet::TaprootWallet,
};
use oracle::mock::MockOracle;
use tokio::sync::mpsc::{self, unbounded_channel};
use types::{
    errors::{self, NetworkError},
    intents::DepositIntent,
    network_event::{DirectMessage, NetworkEvent, SelfRequest, SelfResponse},
};

// Import MockDb from our mocks module
use crate::mocks::db::MockDb;

use crate::util::local_dkg::perform_distributed_key_generation;

type MockNodeState = NodeState<MockNetwork, MockDb, TaprootWallet>;

#[derive(Debug)]
pub struct SenderToNode {
    pub pending_events: Vec<NetworkEvent>,
    events_emitter_tx: crossbeam_channel::Sender<NetworkEvent>,
}

impl SenderToNode {
    fn new(events_emitter_tx: crossbeam_channel::Sender<NetworkEvent>) -> Self {
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
pub struct MockNetwork {
    pub peer: libp2p::PeerId,
    pub events_emitter_tx: crossbeam_channel::Sender<NetworkEvent>,
    pub pending_events_tx: mpsc::UnboundedSender<PendingNetworkEvent>,
}

impl MockNetwork {
    pub fn new(
        events_emitter_tx: crossbeam_channel::Sender<NetworkEvent>,
        peer: libp2p::PeerId,
        pending_events_tx: mpsc::UnboundedSender<PendingNetworkEvent>,
    ) -> Self {
        Self {
            events_emitter_tx,
            peer,
            pending_events_tx,
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

        self.pending_events_tx
            .send(pending_event)
            .map_err(|_| NetworkError::SendError("Failed to send pending event".to_string()))?;
        Ok(())
    }

    fn send_private_message(
        &self,
        peer_id: libp2p::PeerId,
        request: DirectMessage,
    ) -> Result<(), errors::NetworkError> {
        // For mock purposes, we'll create a simplified message event
        // In a real implementation, this would use proper request-response channels
        let pending_event = PendingNetworkEvent {
            from_peer: self.peer,
            event: NetworkEvent::MessageEvent((self.peer_id(), request)),
            target_peers: vec![peer_id],
        };

        self.pending_events_tx
            .send(pending_event)
            .map_err(|_| NetworkError::SendError("Failed to send pending event".to_string()))?;
        Ok(())
    }

    fn send_self_request(
        &self,
        request: SelfRequest,
        sync: bool,
    ) -> Result<Option<NetworkResponseFuture>, errors::NetworkError> {
        if sync {
            let (tx, mut rx) = unbounded_channel::<SelfResponse>();

            let network_message = NetworkEvent::SelfRequest {
                request,
                response_channel: Some(tx),
            };

            self.events_emitter_tx
                .send(network_message)
                .map_err(|e| NetworkError::SendError(e.to_string()))?;

            Ok(Some(Box::pin(async move {
                rx.recv().await.ok_or(NetworkError::RecvError)
            })))
        } else {
            let network_message = NetworkEvent::SelfRequest {
                request,
                response_channel: None,
            };

            self.events_emitter_tx
                .send(network_message)
                .map_err(|e| NetworkError::SendError(e.to_string()))?;

            Ok(None)
        }
    }
}

pub struct MockNodeCluster {
    pub nodes: BTreeMap<libp2p::PeerId, MockNodeState>,
    pub senders: BTreeMap<libp2p::PeerId, SenderToNode>,
    pub networks: BTreeMap<libp2p::PeerId, MockNetwork>,
    pub pending_events_rx: mpsc::UnboundedReceiver<PendingNetworkEvent>,
}

impl MockNodeCluster {
    pub async fn new(peers: u32, min_signers: u16, max_signers: u16) -> Self {
        let mut path = PathBuf::new();
        path.push("config.json");

        let mut config_path = PathBuf::new();
        config_path.push("config.toml");

        let node_config = node::NodeConfig::new(path.clone(), config_path, None, "test-password")
            .expect("Failed to create node config");

        let mut nodes = BTreeMap::new();
        let mut senders = BTreeMap::new();
        let mut networks = BTreeMap::new();

        // Create a single channel for all pending events
        let (pending_events_tx, pending_events_rx) = mpsc::unbounded_channel();

        for _i in 0..peers {
            let peer_id = libp2p::PeerId::random();
            let Ok((node, network)) = create_node_network(
                peer_id,
                node_config.clone(),
                min_signers,
                max_signers,
                pending_events_tx.clone(),
            ) else {
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
            pending_events_rx,
        }
    }

    pub async fn setup(&mut self) {
        // Set environment variable for testing
        #[allow(clippy::missing_safety_doc)]
        unsafe {
            std::env::set_var("KEY_PASSWORD", "test-password");
        }

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
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    // Process network events that were generated during node polling
    async fn process_network_events(&mut self) {
        // Collect all pending events from the channel
        let mut all_pending_events = Vec::new();

        // Drain all available events from the channel
        while let Ok(event) = self.pending_events_rx.try_recv() {
            all_pending_events.push(event);
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
                        NetworkEvent::SelfRequest { request, .. } => NetworkEvent::SelfRequest {
                            request: request.clone(),
                            response_channel: None,
                        },
                        NetworkEvent::Subscribed { peer_id, topic } => NetworkEvent::Subscribed {
                            peer_id: *peer_id,
                            topic: topic.clone(),
                        },
                        NetworkEvent::MessageEvent((peer, message)) => {
                            NetworkEvent::MessageEvent((*peer, message.clone()))
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
                        NetworkEvent::MessageEvent((peer, message)) => {
                            NetworkEvent::MessageEvent((*peer, message.clone()))
                        }
                        _ => {
                            continue;
                        }
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

    pub fn send_self_request_to_peer(&mut self, peer_id: libp2p::PeerId, request: SelfRequest) {
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

    pub async fn new_with_keys(peers: u32, min_signers: u16, max_signers: u16) -> Self {
        let mut cluster = Self::new(peers, min_signers, max_signers).await;
        let identifiers: Vec<Identifier> = cluster
            .nodes
            .keys()
            .map(node::peer_id_to_identifier)
            .collect();

        // Run offline DKG once and distribute keys
        let dkg_out =
            perform_distributed_key_generation(identifiers, max_signers, min_signers).unwrap();

        for (peer_id, node) in cluster.nodes.iter_mut() {
            let id = node::peer_id_to_identifier(peer_id);
            let key_pkg = dkg_out
                .key_packages
                .get(&id)
                .expect("missing key package")
                .clone();
            node.private_key_package = Some(key_pkg);
            node.pubkey_package = Some(dkg_out.pubkey_package.clone());
        }

        cluster
    }
}

pub fn create_node_network(
    peer_id: libp2p::PeerId,
    node_config: node::NodeConfig,
    min_signers: u16,
    max_signers: u16,
    pending_events_tx: mpsc::UnboundedSender<PendingNetworkEvent>,
) -> Result<(MockNodeState, MockNetwork), errors::NodeError> {
    let (events_emitter_tx, events_emitter_rx) = crossbeam_channel::bounded::<NetworkEvent>(256);
    let (deposit_intent_tx, deposit_intent_rx) = crossbeam_channel::bounded::<DepositIntent>(100);

    let network = MockNetwork::new(events_emitter_tx.clone(), peer_id, pending_events_tx);

    let mock_db = MockDb::new();
    let oracle = MockOracle::new(events_emitter_tx.clone(), Some(deposit_intent_rx.clone()));

    let wallet = TaprootWallet::new(
        Box::new(oracle.clone()),
        Vec::new(),
        bitcoin::network::Network::Testnet,
    );

    let nodes_state = NodeState::new_from_config(
        network.clone(),
        min_signers,
        max_signers,
        node_config,
        mock_db,
        events_emitter_rx,
        deposit_intent_tx,
        Box::new(oracle),
        wallet,
    )?;

    Ok((nodes_state, network))
}

#[cfg(test)]
mod node_tests {
    use super::*;

    #[tokio::test]
    async fn peers_can_connect() {
        let mut cluster = MockNodeCluster::new(2, 2, 2).await;
        cluster.setup().await;
        println!("Ran setup");
        cluster.run_n_iterations(1).await;
        println!("Ran 1 iterations");

        for (_, node) in cluster.nodes.iter() {
            assert_eq!(node.peers.len(), 1);
        }

        cluster.tear_down().await;
        println!("Ran teardown");
    }

    #[tokio::test]
    async fn network_events_are_processed_correctly() {
        let mut cluster = MockNodeCluster::new(3, 2, 3).await;
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
        }

        // Process the events
        cluster.process_network_events().await;

        // Check that events were queued in the appropriate senders
        let second_sender = cluster.senders.get(&second_peer).unwrap();
        assert!(
            !second_sender.pending_events.is_empty(),
            "Second peer should have received events"
        );

        println!("Network event processing works correctly!");
    }
}
