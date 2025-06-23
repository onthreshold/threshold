use tonic::{Request, Response, Status};
use types::network::network_protocol::NetworkHandle;

use crate::{grpc_operator, route_metrics};

use types::proto::node_proto::{
    CheckBalanceRequest, CheckBalanceResponse, ConfirmWithdrawalRequest, ConfirmWithdrawalResponse,
    CreateDepositIntentRequest, CreateDepositIntentResponse, GetChainInfoRequest,
    GetChainInfoResponse, GetLatestBlocksRequest, GetLatestBlocksResponse,
    GetPendingDepositIntentsRequest, GetPendingDepositIntentsResponse, ProposeWithdrawalRequest,
    ProposeWithdrawalResponse, SpendFundsRequest, SpendFundsResponse, StartSigningRequest,
    StartSigningResponse, TriggerConsensusRoundRequest, TriggerConsensusRoundResponse,
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
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::spend_funds(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn start_signing(
        &self,
        request: Request<StartSigningRequest>,
    ) -> Result<Response<StartSigningResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::start_signing(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn create_deposit_intent(
        &self,
        request: Request<CreateDepositIntentRequest>,
    ) -> Result<Response<CreateDepositIntentResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::create_deposit_intent(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn get_pending_deposit_intents(
        &self,
        _request: Request<GetPendingDepositIntentsRequest>,
    ) -> Result<Response<GetPendingDepositIntentsResponse>, Status> {
        route_metrics!(async {
            let resp = grpc_operator::get_pending_deposit_intents(&self.network).await?;
            Ok(Response::new(resp))
        })
    }

    async fn propose_withdrawal(
        &self,
        request: Request<ProposeWithdrawalRequest>,
    ) -> Result<Response<ProposeWithdrawalResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::propose_withdrawal(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn confirm_withdrawal(
        &self,
        request: Request<ConfirmWithdrawalRequest>,
    ) -> Result<Response<ConfirmWithdrawalResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::confirm_withdrawal(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn check_balance(
        &self,
        request: Request<CheckBalanceRequest>,
    ) -> Result<Response<CheckBalanceResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::check_balance(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn get_chain_info(
        &self,
        request: Request<GetChainInfoRequest>,
    ) -> Result<Response<GetChainInfoResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::get_chain_info(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn trigger_consensus_round(
        &self,
        request: Request<TriggerConsensusRoundRequest>,
    ) -> Result<Response<TriggerConsensusRoundResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::trigger_consensus_round(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }

    async fn get_latest_blocks(
        &self,
        request: Request<GetLatestBlocksRequest>,
    ) -> Result<Response<GetLatestBlocksResponse>, Status> {
        route_metrics!(async {
            let req = request.into_inner();
            let resp = grpc_operator::get_latest_blocks(&self.network, req).await?;
            Ok(Response::new(resp))
        })
    }
}
