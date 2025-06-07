use std::{collections::BTreeMap, path::PathBuf};

use node::{
    NodeState,
    db::{Db, RocksDb},
    swarm_manager::{Network, NetworkEvent, NetworkResponseFuture},
};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use types::errors;

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

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MockNetwork {
    peer: libp2p::PeerId,
    events_emitter_tx: UnboundedSender<NetworkEvent>,
}

impl MockNetwork {
    pub fn new(events_emitter_tx: UnboundedSender<NetworkEvent>, peer: libp2p::PeerId) -> Self {
        Self {
            events_emitter_tx,
            peer,
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
        todo!("Implement send_broadcast")
    }

    fn send_private_message(
        &self,
        peer_id: libp2p::PeerId,
        request: node::swarm_manager::DirectMessage,
    ) -> Result<(), errors::NetworkError> {
        todo!("Implement send_private_request")
    }

    fn send_self_request(
        &self,
        request: node::swarm_manager::SelfRequest,
        sync: bool,
    ) -> Result<Option<NetworkResponseFuture>, errors::NetworkError> {
        todo!("Implement send_self_request")
    }
}

pub struct MockNodeCluster {
    nodes: BTreeMap<libp2p::PeerId, NodeState<MockNetwork, RocksDb>>,
    senders: BTreeMap<libp2p::PeerId, SenderToNode>,
}

impl MockNodeCluster {
    pub async fn new(peers: u32, min_signers: u16, max_signers: u16) -> Self {
        let mut path = PathBuf::new();
        path.push("config.json");

        let mut config_path = PathBuf::new();
        config_path.push("config.toml");

        let node_config = node::NodeConfig::new(path.clone(), config_path, None);

        let mut nodes = BTreeMap::new();
        let mut senders = BTreeMap::new();

        for i in 0..peers {
            let peer_id = libp2p::PeerId::random();
            let Ok((node, network)) = create_node_network(
                peer_id,
                node_config.clone(),
                min_signers,
                max_signers,
                RocksDb::new(format!("node-{}", i).as_str()),
            ) else {
                panic!("Failed to create node network");
            };

            nodes.insert(peer_id, node);
            senders.insert(
                peer_id,
                SenderToNode::new(network.events_emitter_tx.clone()),
            );
        }

        Self { nodes, senders }
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
                node.poll().await.unwrap();
            }


        }
    }
}

pub fn create_node_network<D: Db>(
    peer_id: libp2p::PeerId,
    node_config: node::NodeConfig,
    min_signers: u16,
    max_signers: u16,
    db: D,
) -> Result<(NodeState<MockNetwork, D>, MockNetwork), errors::NodeError> {
    let (events_emitter_tx, events_emitter_rx) = unbounded_channel::<NetworkEvent>();
    let network = MockNetwork {
        events_emitter_tx,
        peer: peer_id,
    };

    let nodes_state = NodeState::new_from_config(
        network.clone(),
        min_signers,
        max_signers,
        node_config,
        db,
        events_emitter_rx,
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
}
