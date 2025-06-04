use node::grpc_service::node_proto::{
    self, node_control_client::NodeControlClient, SpendFundsResponse, StartSigningResponse, SendDirectMessageResponse
};
use tonic::Status;

pub async fn rpc_spend(
    endpoint: Option<String>,
    amount: u64,
) -> Result<SpendFundsResponse, Status> {
    println!("Spending {} satoshis", amount);

    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or("http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let spendfunds_response = client
        .spend_funds(tonic::Request::new(node_proto::SpendFundsRequest {
            amount_satoshis: amount,
        }))
        .await?;

    println!("Spent {:?} satoshis", spendfunds_response);

    Ok(spendfunds_response.into_inner())
}

pub async fn rpc_start_signing(
    endpoint: Option<String>,
    hex_message: String,
) -> Result<StartSigningResponse, Status> {
    println!("Starting signing session for message: {}", hex_message);

    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or("http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let start_signing_response = client
        .start_signing(tonic::Request::new(node_proto::StartSigningRequest {
            hex_message: hex_message,
        }))
        .await?;

    Ok(start_signing_response.into_inner())
}

pub async fn rpc_send_direct_message(
    endpoint: Option<String>,
    peer_id: String,
    message: String,
) -> Result<SendDirectMessageResponse, Status> {
    println!("Sending direct message to {}: {}", peer_id, message);

    let mut client =
        NodeControlClient::connect(endpoint.unwrap_or("http://[::1]:50051".to_string()))
            .await
            .expect("Failed to connect");

    let send_direct_message_response = client
        .send_direct_message(tonic::Request::new(node_proto::SendDirectMessageRequest {
            peer_id: peer_id,
            message: message,
        }))
        .await?;

    Ok(send_direct_message_response.into_inner())
}

