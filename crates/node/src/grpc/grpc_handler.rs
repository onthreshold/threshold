use crate::swarm_manager::NetworkHandle;
use tonic::{Request, Response, Status};

use crate::grpc::grpc_operator;

// Include the generated proto code
pub mod node_proto {
    tonic::include_proto!("node");
}

use node_proto::{
    CheckBalanceRequest, CheckBalanceResponse, ConfirmWithdrawalRequest, ConfirmWithdrawalResponse,
    CreateDepositIntentRequest, CreateDepositIntentResponse, GetPendingDepositIntentsRequest,
    GetPendingDepositIntentsResponse, ProposeWithdrawalRequest, ProposeWithdrawalResponse,
    SpendFundsRequest, SpendFundsResponse, StartSigningRequest, StartSigningResponse,
    node_control_server::{NodeControl, NodeControlServer},
};

pub struct NodeControlService {
    network: NetworkHandle,
}

impl NodeControlService {
    #[must_use]
    pub const fn new(network: NetworkHandle) -> Self {
        Self { network }
    }

    #[must_use]
    pub fn into_server(self) -> NodeControlServer<Self> {
        NodeControlServer::new(self)
    }
}

#[tonic::async_trait]
impl NodeControl for NodeControlService {
    async fn spend_funds(
        &self,
        request: Request<SpendFundsRequest>,
    ) -> Result<Response<SpendFundsResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::spend_funds(&self.network, request).await?;

        Ok(Response::new(response))
    }

    async fn start_signing(
        &self,
        request: Request<StartSigningRequest>,
    ) -> Result<Response<StartSigningResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::start_signing(&self.network, request).await?;

        Ok(Response::new(response))
    }

    async fn create_deposit_intent(
        &self,
        request: Request<CreateDepositIntentRequest>,
    ) -> Result<Response<CreateDepositIntentResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::create_deposit_intent(&self.network, request).await?;

        Ok(Response::new(response))
    }

    async fn get_pending_deposit_intents(
        &self,
        _request: Request<GetPendingDepositIntentsRequest>,
    ) -> Result<Response<GetPendingDepositIntentsResponse>, Status> {
        let response = grpc_operator::get_pending_deposit_intents(&self.network).await?;
        Ok(Response::new(response))
    }

    async fn propose_withdrawal(
        &self,
        request: Request<ProposeWithdrawalRequest>,
    ) -> Result<Response<ProposeWithdrawalResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::propose_withdrawal(&self.network, request).await?;
        Ok(Response::new(response))
    }

    async fn confirm_withdrawal(
        &self,
        request: Request<ConfirmWithdrawalRequest>,
    ) -> Result<Response<ConfirmWithdrawalResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::confirm_withdrawal(&self.network, request).await?;
        Ok(Response::new(response))
    }

    async fn check_balance(
        &self,
        request: Request<CheckBalanceRequest>,
    ) -> Result<Response<CheckBalanceResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::check_balance(&self.network, request).await?;
        Ok(Response::new(response))
    }
}
