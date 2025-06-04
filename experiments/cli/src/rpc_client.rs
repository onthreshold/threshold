use node::grpc_service::node_proto::{
    self, node_control_client::NodeControlClient, SpendFundsResponse,
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

    Ok(spendfunds_response.into_inner())
}
