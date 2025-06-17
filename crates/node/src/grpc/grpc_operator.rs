use crate::grpc::grpc_handler::node_proto::{
    self, CheckBalanceRequest, CheckBalanceResponse, ConfirmWithdrawalRequest,
    ConfirmWithdrawalResponse, CreateDepositIntentRequest, CreateDepositIntentResponse,
    GetPendingDepositIntentsResponse, ProposeWithdrawalRequest, ProposeWithdrawalResponse,
    SpendFundsRequest, SpendFundsResponse, StartSigningRequest, StartSigningResponse,
};
use crate::swarm_manager::{Network, NetworkHandle};
use tonic::Status;
use tracing::{debug, info};
use types::intents::WithdrawlIntent;
use types::network_event::{SelfRequest, SelfResponse};

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
                fee: 200,
                address_to,
                user_pubkey: "".to_string(),
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

    let response = network
        .send_self_request(
            SelfRequest::CreateDeposit {
                user_pubkey: req.public_key,
                amount_sat,
            },
            true,
        )
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::CreateDepositResponse {
        deposit_tracking_id,
        deposit_address,
    } = response
    else {
        return Err(Status::internal("Invalid response from node"));
    };

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

pub async fn propose_withdrawal(
    network: &impl Network,
    request: ProposeWithdrawalRequest,
) -> Result<ProposeWithdrawalResponse, Status> {
    let amount_sat = if request.amount_satoshis > 0 {
        request.amount_satoshis
    } else {
        return Err(Status::invalid_argument(
            "Amount to withdraw must be greater than 0",
        ));
    };

    let withdrawal_intent = WithdrawlIntent {
        amount_sat,
        address_to: request.address_to,
        public_key: request.public_key,
        blocks_to_confirm: request.blocks_to_confirm,
    };

    let response = network
        .send_self_request(SelfRequest::ProposeWithdrawal { withdrawal_intent }, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::ProposeWithdrawalResponse {
        quote_satoshis,
        challenge,
    } = response
    else {
        return Err(Status::internal("Invalid response from node"));
    };

    Ok(ProposeWithdrawalResponse {
        quote_satoshis,
        challenge,
    })
}

pub async fn confirm_withdrawal(
    network: &impl Network,
    request: ConfirmWithdrawalRequest,
) -> Result<ConfirmWithdrawalResponse, Status> {
    let challenge = request.challenge;
    let signature = request.signature;

    let response = network
        .send_self_request(
            SelfRequest::ConfirmWithdrawal {
                challenge,
                signature,
            },
            true,
        )
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::ConfirmWithdrawalResponse { success } = response else {
        return Err(Status::internal("Invalid response from node"));
    };

    Ok(ConfirmWithdrawalResponse { success })
}

pub async fn check_balance(
    network: &impl Network,
    request: CheckBalanceRequest,
) -> Result<CheckBalanceResponse, Status> {
    let address = request.address;

    let response = network
        .send_self_request(SelfRequest::CheckBalance { address }, true)
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?
        .ok_or(Status::internal("No response from node"))?
        .await
        .map_err(|e| Status::internal(format!("Network error: {:?}", e)))?;

    let SelfResponse::CheckBalanceResponse { balance_satoshis } = response else {
        return Err(Status::internal("Invalid response from node"));
    };

    Ok(CheckBalanceResponse { balance_satoshis })
}
