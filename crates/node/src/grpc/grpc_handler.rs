use crate::swarm_manager::NetworkHandle;
use tonic::{Request, Response, Status};

use crate::grpc::grpc_operator;

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
        grpc_operator::start_dkg(&self.network, _request).await
    }

    async fn spend_funds(
        &self,
        request: Request<SpendFundsRequest>,
    ) -> Result<Response<SpendFundsResponse>, Status> {
        grpc_operator::spend_funds(&self.network, request).await
    }

    async fn start_signing(
        &self,
        request: Request<StartSigningRequest>,
    ) -> Result<Response<StartSigningResponse>, Status> {
        grpc_operator::start_signing(&self.network, request).await
    }

    async fn send_direct_message(
        &self,
        request: Request<SendDirectMessageRequest>,
    ) -> Result<Response<SendDirectMessageResponse>, Status> {
        grpc_operator::send_direct_message(&self.network, request).await
    }

    async fn create_deposit_intent(
        &self,
        request: Request<CreateDepositIntentRequest>,
    ) -> Result<Response<CreateDepositIntentResponse>, Status> {
        let request = request.into_inner();
        let response = grpc_operator::create_deposit_intent(&self.network, request).await?;
        Ok(Response::new(response))
    }
}
