use crate::{Network, NodeState, handlers::Handler, wallet::Wallet};
use abci::chain_state::Account;
use abci::{ChainMessage, ChainResponse};
use types::errors::NodeError;
use types::network::network_event::{BlockInfo, NetworkEvent, SelfRequest, SelfResponse};

#[derive(Default)]
pub struct BalanceState;

impl BalanceState {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for BalanceState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        match message {
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::CheckBalance { address },
                response_channel,
            }) => {
                let ChainResponse::GetAccount { account } = node
                    .chain_interface_tx
                    .send_message_with_response(ChainMessage::GetAccount {
                        address: address.clone(),
                    })
                    .await?
                else {
                    return Err(NodeError::Error("Failed to get account".to_string()));
                };

                let account = account.unwrap_or_else(|| Account::new(address, 0));

                let balance = account.balance;

                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::CheckBalanceResponse {
                            balance_satoshis: balance,
                        })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::GetChainInfo,
                response_channel,
            }) => {
                let ChainResponse::GetChainState { state } = node
                    .chain_interface_tx
                    .send_message_with_response(ChainMessage::GetChainState)
                    .await?
                else {
                    return Err(NodeError::Error("Failed to get chain state".to_string()));
                };

                let latest_height = state.get_block_height();
                let latest_block_hash = "latest".to_string(); // Simple placeholder
                let pending_transactions = state.get_pending_transactions().len() as u64;
                let total_blocks = latest_height + 1; // Simple approximation

                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::GetChainInfoResponse {
                            latest_height,
                            latest_block_hash,
                            pending_transactions,
                            total_blocks,
                        })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::GetLatestBlocks { count: _ },
                response_channel,
            }) => {
                let ChainResponse::GetChainState { state } = node
                    .chain_interface_tx
                    .send_message_with_response(ChainMessage::GetChainState)
                    .await?
                else {
                    return Err(NodeError::Error("Failed to get chain state".to_string()));
                };

                let mut blocks = Vec::new();
                let current_height = state.get_block_height();

                if current_height > 0 {
                    blocks.push(BlockInfo {
                        height: current_height,
                        hash: format!("block_hash_{current_height}"),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        transaction_count: u32::try_from(state.get_pending_transactions().len())
                            .unwrap(),
                    });
                }

                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::GetLatestBlocksResponse { blocks })
                        .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
