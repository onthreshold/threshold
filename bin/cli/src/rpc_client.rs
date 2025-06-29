use tonic::Status;
use types::proto::node_proto::{
    self, CheckBalanceResponse, CreateDepositIntentResponse, GetPendingDepositIntentsResponse,
    SpendFundsResponse, StartSigningResponse, node_control_client::NodeControlClient,
};

pub async fn rpc_spend(
    endpoint: Option<String>,
    amount: u64,
    address_to: String,
) -> Result<SpendFundsResponse, Status> {
    println!("Spending {amount} satoshis");

    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or_else(|| "http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let spendfunds_response = client
        .spend_funds(tonic::Request::new(node_proto::SpendFundsRequest {
            amount_satoshis: amount,
            address_to,
        }))
        .await?;

    println!("Spent {spendfunds_response:?} satoshis");

    Ok(spendfunds_response.into_inner())
}

pub async fn rpc_start_signing(
    endpoint: Option<String>,
    hex_message: String,
) -> Result<StartSigningResponse, Status> {
    println!("Starting signing session for message: {hex_message}");

    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or_else(|| "http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let start_signing_response = client
        .start_signing(tonic::Request::new(node_proto::StartSigningRequest {
            hex_message,
        }))
        .await?;

    Ok(start_signing_response.into_inner())
}

pub async fn rpc_create_deposit_intent(
    endpoint: Option<String>,
    amount: u64,
    public_key: String,
) -> Result<CreateDepositIntentResponse, Status> {
    println!("Creating deposit intent: {amount}");

    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or_else(|| "http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let create_deposit_intent_response = client
        .create_deposit_intent(tonic::Request::new(
            node_proto::CreateDepositIntentRequest {
                amount_satoshis: amount,
                public_key,
            },
        ))
        .await?;

    Ok(create_deposit_intent_response.into_inner())
}

pub async fn rpc_get_pending_deposit_intents(
    endpoint: Option<String>,
) -> Result<GetPendingDepositIntentsResponse, Status> {
    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or_else(|| "http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let get_pending_deposit_intents_response = client
        .get_pending_deposit_intents(tonic::Request::new(
            node_proto::GetPendingDepositIntentsRequest {},
        ))
        .await?;

    println!("Deposit intents: {get_pending_deposit_intents_response:?}");

    Ok(get_pending_deposit_intents_response.into_inner())
}

pub async fn rpc_check_balance(
    endpoint: Option<String>,
    address: String,
) -> Result<CheckBalanceResponse, Status> {
    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or_else(|| "http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let check_balance_response = client
        .check_balance(tonic::Request::new(node_proto::CheckBalanceRequest {
            address,
        }))
        .await?;

    println!("Balance: {check_balance_response:?}");

    Ok(check_balance_response.into_inner())
}
