use crate::deposit_intents::DepositIntent;
use crate::grpc::grpc_handler::node_proto::{
    self, CreateDepositIntentRequest, CreateDepositIntentResponse,
    GetPendingDepositIntentsResponse, SpendFundsRequest, SpendFundsResponse, StartSigningRequest,
    StartSigningResponse,
};
use crate::swarm_manager::{Network, NetworkHandle, SelfRequest, SelfResponse};
use bitcoin::Address;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Scalar;
use libp2p::gossipsub::IdentTopic;
use serde_json;
use std::str::FromStr;
use tonic::Status;
use tracing::{debug, info};
use uuid::Uuid;

pub async fn spend_funds(
    network: &NetworkHandle,
    request: SpendFundsRequest,
) -> Result<SpendFundsResponse, Status> {
    let amount_sat = request.amount_satoshis;
    let address_to = request.address_to;

    debug!("Received request to spend {} satoshis", amount_sat);
    let response = network
        .send_self_request(
            SelfRequest::Spend {
                amount_sat,
                address_to,
            },
            true,
        )
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::SpendRequestSent { sighash } = response else {
        return Err(Status::internal("Invalid response from node"));
    };

    Ok(SpendFundsResponse {
        success: true,
        message: format!("Spending {} satoshis", amount_sat),
        sighash: sighash.to_string(),
    })
}

pub async fn start_signing(
    network: &NetworkHandle,
    request: StartSigningRequest,
) -> Result<StartSigningResponse, Status> {
    let hex_msg = request.hex_message;

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

    Ok(StartSigningResponse {
        success: true,
        message: "Signing session started".to_string(),
        sign_id,
    })
}

pub async fn create_deposit_intent(
    network: &impl Network,
    request: CreateDepositIntentRequest,
) -> Result<CreateDepositIntentResponse, Status> {
    let req = request;

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

    let secp = bitcoin::secp256k1::Secp256k1::new();

    let internal_key = public_key.inner.x_only_public_key().0;

    let tweak_scalar = Scalar::from_be_bytes(
        bitcoin::hashes::sha256::Hash::hash(deposit_tracking_id.as_bytes()).to_byte_array(),
    )
    .expect("32 bytes, should not fail");

    let (tweaked_key, _) = internal_key
        .add_tweak(&secp, &tweak_scalar)
        .map_err(|e| Status::internal(format!("Failed to add tweak: {:?}", e)))?;

    let deposit_address = Address::p2tr(&secp, tweaked_key, None, bitcoin::Network::Testnet);

    let deposit_intent = DepositIntent {
        amount_sat,
        deposit_tracking_id: deposit_tracking_id.clone(),
        deposit_address: deposit_address.to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    let _ = network
        .send_self_request(SelfRequest::CreateDeposit { deposit_intent }, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let broadcast_message = serde_json::json!({
        "deposit_address": deposit_address.to_string(),
        "amount_sat": amount_sat,
        "deposit_tracking_id": deposit_tracking_id,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });

    if let Err(e) = network.send_broadcast(
        IdentTopic::new("deposit-intents"),
        broadcast_message.to_string().as_bytes().to_vec(),
    ) {
        info!("Failed to broadcast new deposit address: {:?}", e);
    }

    info!(
        "Received request to create deposit intent with amount {}. Tracking ID: {}. Deposit Address: {}",
        amount_sat,
        deposit_tracking_id.clone(),
        deposit_address.clone().to_string()
    );

    Ok(CreateDepositIntentResponse {
        success: true,
        message: "Deposit intent created".to_string(),
        deposit_tracking_id,
        deposit_address: deposit_address.to_string(),
    })
}

pub async fn get_pending_deposit_intents(
    network: &impl Network,
) -> Result<GetPendingDepositIntentsResponse, Status> {
    let intents = network
        .send_self_request(SelfRequest::GetPendingDepositIntents, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::GetPendingDepositIntentsResponse { intents } = intents else {
        return Err(Status::internal("Invalid response from node"));
    };

    Ok(GetPendingDepositIntentsResponse {
        intents: intents
            .iter()
            .map(|intent| node_proto::DepositIntent {
                amount_satoshis: intent.amount_sat,
                deposit_tracking_id: intent.deposit_tracking_id.clone(),
                deposit_address: intent.deposit_address.clone(),
                timestamp: intent.timestamp,
            })
            .collect(),
    })
}
