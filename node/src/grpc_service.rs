use libp2p::PeerId;
use libp2p::gossipsub::IdentTopic;
use tonic::{Request, Response, Status};
use crate::swarm_manager::{NetworkHandle, PingBody, PrivateRequest, PrivateResponse};

use uuid::Uuid;
use bitcoin::Network;
use bitcoin::script::Builder;
use bitcoin::PublicKey;
use bitcoin::Address;
use std::str::FromStr;

// Include the generated proto code
pub mod node_proto {
    tonic::include_proto!("node");
}

use node_proto::{
    node_control_server::{NodeControl, NodeControlServer},
    *,
};

pub struct NodeControlService {
    network: NetworkHandle,
}

impl NodeControlService {
    pub fn new(network: NetworkHandle) -> Self {
        Self { network }
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
        // Create start-dkg topic
        let start_dkg_topic = IdentTopic::new("start-dkg");

        // Send a message to start DKG
        let start_message = "START_DKG".to_string();
        self.network
            .send_broadcast(start_dkg_topic.clone(), start_message.as_bytes().to_vec());
        // // Handle DKG start locally
        // node_state.handle_dkg_start();

        Ok(Response::new(StartDkgResponse {
            success: true,
            message: "DKG process started".to_string(),
        }))
    }

    async fn spend_funds(
        &self,
        request: Request<SpendFundsRequest>,
    ) -> Result<Response<SpendFundsResponse>, Status> {
        let amount_sat = request.into_inner().amount_satoshis;

        println!("Received request to spend {} satoshis", amount_sat);
        let response = self
            .network
            .send_self_request(PrivateRequest::Spend { amount_sat })
            .await;

        let Some(PrivateResponse::SpendRequestSent { sighash }) = response else {
            return Err(Status::internal("Invalid response from node"));
        };

        Ok(Response::new(SpendFundsResponse {
            success: true,
            message: format!("Spending {} satoshis", amount_sat),
            sighash: sighash.to_string(),
        }))
    }

    async fn start_signing(
        &self,
        request: Request<StartSigningRequest>,
    ) -> Result<Response<StartSigningResponse>, Status> {
        let hex_msg = request.into_inner().hex_message;

        let network_request = PrivateRequest::StartSigningSession {
            hex_message: hex_msg.clone(),
        };

        let response = self.network.send_self_request(network_request).await;

        let Some(PrivateResponse::StartSigningSession { sign_id }) = response else {
            return Err(Status::internal(format!("Invalid response from node {:?}", response)));
        };

        Ok(Response::new(StartSigningResponse {
            success: true,
            message: "Signing session started".to_string(),
            sign_id,
        }))
    }

    async fn send_direct_message(
        &self,
        request: Request<SendDirectMessageRequest>,
    ) -> Result<Response<SendDirectMessageResponse>, Status> {
        // let mut node_state = self.node_state.lock().await;
        let req = request.into_inner();

        let target_peer_id = req
            .peer_id
            .parse::<PeerId>()
            .map_err(|e| Status::invalid_argument(format!("Invalid peer ID: {}", e)))?;

        let direct_message = format!("From: {}", req.message);

        self.network.send_private_request(
            target_peer_id,
            PrivateRequest::Ping(PingBody {
                message: direct_message,
            }),
        );

        Ok(Response::new(SendDirectMessageResponse {
            success: true,
            message: format!("Message sent to {}", target_peer_id),
        }))
    }

    async fn create_deposit_intent(
        &self,
        request: Request<CreateDepositIntentRequest>,
    ) -> Result<Response<CreateDepositIntentResponse>, Status> {
        let req = request.into_inner();

        let user_id = if req.user_id.parse::<PeerId>().is_ok() {
            req.user_id
        } else {
            return Err(Status::invalid_argument("User ID must be a valid peer ID"));
        };

        let amount_sat = if req.amount_satoshis > 0 {
            req.amount_satoshis
        } else {
            return Err(Status::invalid_argument("Amount to deposit must be greater than 0"));
        };


        let deposit_tracking_id = Uuid::new_v4().to_string();
        
        // USING MOCK PUBLIC KEY FOR NOW
        let frost_pubkey_hex = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

        let public_key = PublicKey::from_str(frost_pubkey_hex)
            .map_err(|e| Status::internal(format!("Failed to parse public key: {}", e)))?;

        let witness_script = Builder::new()
            .push_key(&public_key)
            .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
            .into_script();

        let deposit_address = Address::p2wsh(&witness_script, Network::Testnet);

        println!("Received request to create deposit intent for user {} with amount {}. Tracking ID: {}. Deposit Address: {}", user_id, amount_sat, deposit_tracking_id.clone(), deposit_address.to_string());

        Ok(Response::new(CreateDepositIntentResponse {
            success: true,
            message: format!("Deposit intent created for user {}", user_id),
            deposit_tracking_id: deposit_tracking_id,
            deposit_address: deposit_address.to_string(),
        }))
    }
}
