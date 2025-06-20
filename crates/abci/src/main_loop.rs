use tokio::sync::broadcast;
use tracing::error;
use types::errors::NodeError;

use crate::{ChainInterface, ChainInterfaceImpl, ChainMessage, ChainResponse};

impl ChainInterfaceImpl {
    pub async fn try_poll(&mut self) -> Result<bool, NodeError> {
        let send_message = self.message_stream.try_recv().ok();
        if let Some(event) = send_message {
            self.handle(Some(event)).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn poll(&mut self) -> Result<(), NodeError> {
        let send_message = self.message_stream.recv().await.ok();
        self.handle(send_message).await
    }

    pub async fn start(&mut self) {
        loop {
            if let Err(e) = self.poll().await {
                error!("Error polling chain messages: {}", e);
            }
        }
    }

    pub async fn handle(
        &mut self,
        send_message: Option<(ChainMessage, broadcast::Sender<ChainResponse>)>,
    ) -> Result<(), NodeError> {
        if let Some((message, response_tx)) = send_message {
            let response = match message {
                ChainMessage::InsertDepositIntent { intent } => {
                    ChainResponse::InsertDepositIntent {
                        error: self.insert_deposit_intent(intent).err(),
                    }
                }
                ChainMessage::GetAccount { address } => ChainResponse::GetAccount {
                    account: self.get_account(&address),
                },
                ChainMessage::GetAllDepositIntents => ChainResponse::GetAllDepositIntents {
                    intents: self.get_all_deposit_intents()?,
                },
                ChainMessage::GetDepositIntentByAddress { address } => {
                    ChainResponse::GetDepositIntentByAddress {
                        intent: self.get_deposit_intent_by_address(&address),
                    }
                }
                ChainMessage::CreateGenesisBlock {
                    validators,
                    chain_config,
                    pubkey,
                } => ChainResponse::CreateGenesisBlock {
                    error: self
                        .create_genesis_block(validators, chain_config, &pubkey)
                        .err(),
                },
                ChainMessage::AddTransactionToBlock { transaction } => {
                    ChainResponse::AddTransactionToBlock {
                        error: self.add_transaction_to_block(transaction).await.err(),
                    }
                }
                ChainMessage::GetProposedBlock {
                    previous_block,
                    proposer,
                } => ChainResponse::GetProposedBlock {
                    block: self.get_proposed_block(previous_block, proposer)?,
                },
            };
            response_tx
                .send(response)
                .map_err(|e| NodeError::Error(format!("Failed to send response: {e}")))?;
        }

        Ok(())
    }
}
