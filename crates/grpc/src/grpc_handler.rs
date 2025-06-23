use tonic::{Request, Response, Status};
use types::network::network_protocol::NetworkHandle;

use crate::grpc_operator;

use types::proto::node_proto::{
    CheckBalanceRequest, CheckBalanceResponse, ConfirmWithdrawalRequest, ConfirmWithdrawalResponse,
    CreateDepositIntentRequest, CreateDepositIntentResponse, GetPendingDepositIntentsRequest,
    GetPendingDepositIntentsResponse, ProposeWithdrawalRequest, ProposeWithdrawalResponse,
    SpendFundsRequest, SpendFundsResponse, StartSigningRequest, StartSigningResponse,
    GetChainInfoRequest, GetChainInfoResponse, TriggerConsensusRoundRequest, TriggerConsensusRoundResponse,
    GetLatestBlocksRequest, GetLatestBlocksResponse,
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

    async fn get_chain_info(
        &self,
        request: Request<GetChainInfoRequest>,
    ) -> Result<Response<GetChainInfoResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::get_chain_info(&self.network, request).await?;
        Ok(Response::new(response))
    }

    async fn trigger_consensus_round(
        &self,
        request: Request<TriggerConsensusRoundRequest>,
    ) -> Result<Response<TriggerConsensusRoundResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::trigger_consensus_round(&self.network, request).await?;
        Ok(Response::new(response))
    }

    async fn get_latest_blocks(
        &self,
        request: Request<GetLatestBlocksRequest>,
    ) -> Result<Response<GetLatestBlocksResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::get_latest_blocks(&self.network, request).await?;
        Ok(Response::new(response))
    }
}
