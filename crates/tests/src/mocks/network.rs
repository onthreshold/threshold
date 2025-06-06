use std::{collections::BTreeMap, path::PathBuf};

use node::{
    NodeState,
    db::{Db, RocksDb},
    swarm_manager::{Network, NetworkEvent, NetworkResponseFuture},
};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use types::errors::{self, NodeError};

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

#[allow(dead_code, unused_variables)]
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

    fn send_private_request(
        &self,
        peer_id: libp2p::PeerId,
        request: node::swarm_manager::PrivateRequest,
    ) -> Result<(), errors::NetworkError> {
        todo!("Implement send_private_request")
    }

    fn send_private_response(
        &self,
        channel: libp2p::request_response::ResponseChannel<node::swarm_manager::PrivateResponse>,
        response: node::swarm_manager::PrivateResponse,
    ) -> Result<(), errors::NetworkError> {
        todo!("Implement send_private_response")
    }

    fn send_self_request(
        &self,
        request: node::swarm_manager::PrivateRequest,
        sync: bool,
    ) -> Result<Option<NetworkResponseFuture>, errors::NetworkError> {
        todo!("Implement send_self_request")
    }
}

#[allow(dead_code)]
pub struct MockNodeCluster {
    node_handles: BTreeMap<libp2p::PeerId, tokio::task::JoinHandle<Result<(), NodeError>>>,
    networks: BTreeMap<libp2p::PeerId, MockNetwork>,
}

impl MockNodeCluster {
    pub async fn new(peers: u32, min_signers: u16, max_signers: u16) -> Self {
        let mut path = PathBuf::new();
        path.push("config.json");

        let node_config = node::NodeConfig::new(path, None);

        let mut networks = BTreeMap::new();
        let mut nodes = BTreeMap::new();
        let mut node_handles = BTreeMap::new();

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

            networks.insert(peer_id, network);
            nodes.insert(peer_id, node);
        }

        for (peer_id, mut node) in nodes {
            let handle = tokio::spawn(async move { node.start().await });

            node_handles.insert(peer_id, handle);
        }

        Self {
            node_handles,
            networks,
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
    async fn create_nodes_with_mock_network() {
        MockNodeCluster::new(2, 2, 2).await;
        println!("Able to create mock node");
    }
}
