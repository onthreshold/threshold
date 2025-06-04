use libp2p::PeerId;
use libp2p::gossipsub::IdentTopic;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::NodeState;
use crate::swarm_manager::{PingBody, PrivateRequest};

// Include the generated proto code
pub mod node_proto {
    tonic::include_proto!("node");
}

use node_proto::{
    node_control_server::{NodeControl, NodeControlServer},
    *,
};

pub struct NodeControlService {
    node_state: Arc<Mutex<NodeState>>,
}

impl NodeControlService {
    pub fn new(node_state: Arc<Mutex<NodeState>>) -> Self {
        Self { node_state }
    }

    pub fn into_server(self) -> NodeControlServer<Self> {
        NodeControlServer::new(self)
    }
}

#[tonic::async_trait]
impl NodeControl for NodeControlService {
    async fn start_dkg(
        &self,
        _request: Request<StartDkgRequest>,
    ) -> Result<Response<StartDkgResponse>, Status> {
        let mut node_state = self.node_state.lock().await;

        // Create start-dkg topic
        let start_dkg_topic = IdentTopic::new("start-dkg");

        // Send a message to start DKG
        let start_message = format!("START_DKG:{}", node_state.peer_id);
        if let Err(e) = node_state
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(start_dkg_topic.clone(), start_message.as_bytes())
        {
            return Err(Status::internal(format!(
                "Failed to publish DKG start: {}",
                e
            )));
        }

        // Handle DKG start locally
        node_state.handle_dkg_start();

        Ok(Response::new(StartDkgResponse {
            success: true,
            message: "DKG process started".to_string(),
        }))
    }

    async fn get_peers(
        &self,
        _request: Request<GetPeersRequest>,
    ) -> Result<Response<GetPeersResponse>, Status> {
        let node_state = self.node_state.lock().await;

        let connected_peers: Vec<_> = node_state
            .swarm
            .behaviour()
            .gossipsub
            .all_peers()
            .map(|(peer_id, _)| peer_id.to_string())
            .collect();

        Ok(Response::new(GetPeersResponse {
            peer_ids: connected_peers.clone(),
            total_count: connected_peers.len() as u32,
        }))
    }

    async fn spend_funds(
        &self,
        request: Request<SpendFundsRequest>,
    ) -> Result<Response<SpendFundsResponse>, Status> {
        let mut node_state = self.node_state.lock().await;
        let amount_sat = request.into_inner().amount_satoshis;

        node_state.handle_spend_request(amount_sat);

        Ok(Response::new(SpendFundsResponse {
            success: true,
            message: format!("Spending {} satoshis", amount_sat),
            transaction_id: String::new(), // Will be filled when transaction is created
        }))
    }

    async fn start_signing(
        &self,
        request: Request<StartSigningRequest>,
    ) -> Result<Response<StartSigningResponse>, Status> {
        let mut node_state = self.node_state.lock().await;
        let hex_msg = request.into_inner().hex_message;

        node_state.start_signing_session(&hex_msg);

        Ok(Response::new(StartSigningResponse {
            success: true,
            message: "Signing session started".to_string(),
            sign_id: node_state
                .active_signing
                .as_ref()
                .map(|s| s.sign_id)
                .unwrap_or(0),
        }))
    }

    async fn send_direct_message(
        &self,
        request: Request<SendDirectMessageRequest>,
    ) -> Result<Response<SendDirectMessageResponse>, Status> {
        let mut node_state = self.node_state.lock().await;
        let req = request.into_inner();

        let target_peer_id = req
            .peer_id
            .parse::<PeerId>()
            .map_err(|e| Status::invalid_argument(format!("Invalid peer ID: {}", e)))?;

        let direct_message = format!("From {}: {}", node_state.peer_id, req.message);

        let request_id = node_state
            .swarm
            .behaviour_mut()
            .request_response
            .send_request(
                &target_peer_id,
                PrivateRequest::Ping(PingBody {
                    message: direct_message,
                }),
            );

        Ok(Response::new(SendDirectMessageResponse {
            success: true,
            message: format!("Message sent to {}", target_peer_id),
            request_id: format!("{:?}", request_id),
        }))
    }

    async fn get_node_info(
        &self,
        _request: Request<GetNodeInfoRequest>,
    ) -> Result<Response<GetNodeInfoResponse>, Status> {
        let node_state = self.node_state.lock().await;

        let connected_peers = node_state.swarm.behaviour().gossipsub.all_peers().count() as u32;

        Ok(Response::new(GetNodeInfoResponse {
            peer_id: node_state.peer_id.to_string(),
            min_signers: node_state.min_signers as u32,
            max_signers: node_state.max_signers as u32,
            has_key_package: node_state.private_key_package.is_some(),
            connected_peers,
        }))
    }
}
