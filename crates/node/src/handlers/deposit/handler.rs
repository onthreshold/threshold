use libp2p::gossipsub::Message;
use tracing::info;
use types::broadcast::BroadcastMessage;
use types::errors::NodeError;

use crate::{
    NodeState,
    handlers::{Handler, deposit::DepositIntentState},
    wallet::Wallet,
};

use abci::ChainMessage;
use types::network::network_event::{NetworkEvent, SelfRequest, SelfResponse};
use types::network::network_protocol::Network;
use types::proto::ProtoDecode;

#[async_trait::async_trait]
impl<N: Network, W: Wallet> Handler<N, W> for DepositIntentState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, W>,
        message: NetworkEvent,
    ) -> Result<(), types::errors::NodeError> {
        match message {
            NetworkEvent::SelfRequest {
                request:
                    SelfRequest::CreateDeposit {
                        user_pubkey,
                        amount_sat,
                    },
                response_channel,
            } => {
                println!("Node receveived request to create deposit");
                let response = self.create_deposit(node, &user_pubkey, amount_sat).await;
                if let Some(response_channel) = response_channel {
                    match response {
                        Ok((deposit_tracking_id, deposit_address)) => {
                            response_channel
                                .send(SelfResponse::CreateDepositResponse {
                                    deposit_tracking_id,
                                    deposit_address,
                                })
                                .map_err(|e| {
                                    NodeError::Error(format!("Failed to send response: {e}"))
                                })?;
                            println!("Deposit created");
                        }
                        Err(e) => {
                            println!("Error creating deposit: {e:?}, sending response");
                            response_channel
                                .send(SelfResponse::NodeError(e))
                                .map_err(|e| {
                                    NodeError::Error(format!("Failed to send response: {e}"))
                                })?;
                        }
                    }
                }
            }
            NetworkEvent::SelfRequest {
                request: SelfRequest::GetPendingDepositIntents,
                response_channel,
            } => {
                let response = self.get_pending_deposit_intents(node).await;
                if let Some(response_channel) = response_channel {
                    match response {
                        Ok(intents) => {
                            response_channel
                                .send(SelfResponse::GetPendingDepositIntentsResponse { intents })
                                .map_err(|e| {
                                    NodeError::Error(format!("Failed to send response: {e}"))
                                })?;
                        }
                        Err(e) => {
                            response_channel
                                .send(SelfResponse::NodeError(e))
                                .map_err(|e| {
                                    NodeError::Error(format!("Failed to send response: {e}"))
                                })?;
                        }
                    }
                }
            }
            NetworkEvent::SelfRequest {
                request: SelfRequest::ConfirmDeposit { confirmed_tx },
                ..
            } => {
                if let Err(e) = self.update_user_balance(node, &confirmed_tx).await {
                    info!("❌ Failed to update user balance: {}", e);
                } else {
                    info!(
                        "✅ Successfully processed deposit transaction: {}",
                        confirmed_tx.compute_txid()
                    );
                }
            }
            NetworkEvent::GossipsubMessage(Message { data, .. }) => {
                let broadcast = BroadcastMessage::decode(&data).map_err(|e| {
                    NodeError::Error(format!("Failed to decode broadcast message: {e}"))
                })?;

                // Handle broadcasted transactions
                if let BroadcastMessage::Transaction(transaction_data) = broadcast {
                    match bincode::decode_from_slice::<protocol::transaction::Transaction, _>(
                        &transaction_data,
                        bincode::config::standard(),
                    ) {
                        Ok((transaction, _)) => {
                            info!(
                                "📨 Received broadcasted transaction: {}",
                                hex::encode(transaction.id())
                            );

                            if let Err(e) = node
                                .chain_interface_tx
                                .send_message_with_response(ChainMessage::AddTransactionToBlock {
                                    transaction: transaction.clone(),
                                })
                                .await
                            {
                                info!(
                                    "Failed to add broadcasted transaction to pending pool: {}",
                                    e
                                );
                            } else {
                                info!("✅ Added broadcasted transaction to pending pool");
                            }

                            let tx = transaction.get_deposit_transaction_address()?;
                            node.wallet.ingest_external_tx(&tx)?;
                        }
                        Err(e) => {
                            info!("Failed to decode broadcasted transaction: {}", e);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
