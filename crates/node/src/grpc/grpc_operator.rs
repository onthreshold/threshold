use crate::grpc::grpc_handler::node_proto::{
    CreateDepositIntentRequest, CreateDepositIntentResponse, SendDirectMessageRequest,
    SendDirectMessageResponse, SpendFundsRequest, SpendFundsResponse, StartDkgRequest,
    StartDkgResponse, StartSigningRequest, StartSigningResponse,
};
use crate::swarm_manager::{
    DirectMessage, Network, NetworkHandle, PingBody, SelfRequest, SelfResponse,
};
use libp2p::PeerId;
use libp2p::gossipsub::IdentTopic;
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use bitcoin::script::Builder;
use std::str::FromStr;
use uuid::Uuid;

pub async fn start_dkg(
    network: &NetworkHandle,
    _request: Request<StartDkgRequest>,
) -> Result<Response<StartDkgResponse>, Status> {
    // Create start-dkg topic
    let start_dkg_topic = IdentTopic::new("start-dkg");

    // Send a message to start DKG
    let start_message = "START_DKG".to_string();

    match network.send_broadcast(start_dkg_topic.clone(), start_message.as_bytes().to_vec()) {
        Ok(_) => Ok(Response::new(StartDkgResponse {
            success: true,
            message: "DKG process started".to_string(),
        })),
        Err(e) => Err(Status::internal(format!("Network error: {:?}", e))),
    }
}

pub async fn spend_funds(
    network: &NetworkHandle,
    request: Request<SpendFundsRequest>,
) -> Result<Response<SpendFundsResponse>, Status> {
    let amount_sat = request.into_inner().amount_satoshis;

    debug!("Received request to spend {} satoshis", amount_sat);
    let response = network
        .send_self_request(SelfRequest::Spend { amount_sat }, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::SpendRequestSent { sighash } = response else {
        return Err(Status::internal("Invalid response from node"));
    };

    Ok(Response::new(SpendFundsResponse {
        success: true,
        message: format!("Spending {} satoshis", amount_sat),
        sighash: sighash.to_string(),
    }))
}

pub async fn start_signing(
    network: &NetworkHandle,
    request: Request<StartSigningRequest>,
) -> Result<Response<StartSigningResponse>, Status> {
    let hex_msg = request.into_inner().hex_message;

    let network_request = SelfRequest::StartSigningSession {
        hex_message: hex_msg.clone(),
    };

    let response = network
        .send_self_request(network_request, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::StartSigningSessionResponse { sign_id } = response else {
        return Err(Status::internal(format!(
            "Invalid response from node {:?}",
            response
        )));
    };

    Ok(Response::new(StartSigningResponse {
        success: true,
        message: "Signing session started".to_string(),
        sign_id,
    }))
}

pub async fn send_direct_message(
    network: &NetworkHandle,
    request: Request<SendDirectMessageRequest>,
) -> Result<Response<SendDirectMessageResponse>, Status> {
    let req = request.into_inner();

    let target_peer_id = req
        .peer_id
        .parse::<PeerId>()
        .map_err(|e| Status::invalid_argument(format!("Invalid peer ID: {}", e)))?;

    let direct_message = format!("From: {}", req.message);

    match network.send_private_message(
        target_peer_id,
        DirectMessage::Ping(PingBody {
            message: direct_message,
        }),
    ) {
        Ok(_) => Ok(Response::new(SendDirectMessageResponse {
            success: true,
            message: format!("Message sent to {}", target_peer_id),
        })),
        Err(e) => Err(Status::internal(format!("Network error: {:?}", e))),
    }
}

pub async fn create_deposit_intent(
    network: &NetworkHandle,
    request: Request<CreateDepositIntentRequest>,
) -> Result<Response<CreateDepositIntentResponse>, Status> {
    let req = request.into_inner();

    let user_id = req
        .user_id
        .parse::<PeerId>()
        .map_err(|e| Status::invalid_argument(format!("Invalid peer ID: {}", e)))?;

    let amount_sat = if req.amount_satoshis > 0 {
        req.amount_satoshis
    } else {
        return Err(Status::invalid_argument(
            "Amount to deposit must be greater than 0",
        ));
    };

    let deposit_tracking_id = Uuid::new_v4().to_string();

    let frost_pubkey_hex = network
        .send_self_request(SelfRequest::GetFrostPublicKey, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::GetFrostPublicKeyResponse {
        public_key: Some(public_key),
    } = frost_pubkey_hex
    else {
        return Err(Status::internal(
            "Invalid response from node. No public key found.",
        ));
    };

    let public_key = bitcoin::PublicKey::from_str(&public_key)
        .map_err(|e| Status::internal(format!("Failed to parse public key: {}", e)))?;

    let witness_script = Builder::new()
        .push_key(&public_key)
        .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
        .into_script();

    let deposit_address = bitcoin::Address::p2wsh(&witness_script, bitcoin::Network::Testnet);

    info!(
        "Received request to create deposit intent for user {} with amount {}. Tracking ID: {}. Deposit Address: {}",
        user_id,
        amount_sat,
        deposit_tracking_id.clone(),
        deposit_address
    );

    Ok(Response::new(CreateDepositIntentResponse {
        success: true,
        message: format!("Deposit intent created for user {}", user_id),
        deposit_tracking_id,
        deposit_address: deposit_address.to_string(),
    }))
}
