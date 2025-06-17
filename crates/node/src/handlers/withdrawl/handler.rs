use crate::swarm_manager::Network;
use crate::wallet::PendingSpend;
use crate::wallet::Wallet;
use crate::{NodeState, db::Db, handlers::Handler, handlers::withdrawl::SpendIntentState};
use libp2p::gossipsub::Message;
use prost::Message as ProstMessage;
use types::errors::NodeError;
use types::network_event::{NetworkEvent, SelfRequest, SelfResponse};

pub fn encode_withdrawal_intent(intent: &PendingSpend) -> Result<Vec<u8>, String> {
    let transaction_bytes = bitcoin::consensus::encode::serialize(&intent.tx);
    let script_bytes = intent.recipient_script.to_bytes();

    let proto_intent = crate::swarm_manager::p2p_proto::PendingSpend {
        transaction: transaction_bytes,
        user_pubkey: intent.user_pubkey.clone(),
        recipient_script: script_bytes,
        fee: intent.fee,
    };

    let mut buf = Vec::new();
    <crate::swarm_manager::p2p_proto::PendingSpend as ProstMessage>::encode(
        &proto_intent,
        &mut buf,
    )
    .map_err(|e| format!("Failed to encode withdrawal intent: {}", e))?;
    Ok(buf)
}

fn decode_withdrawal_intent(data: &[u8]) -> Result<PendingSpend, String> {
    let proto_intent =
        <crate::swarm_manager::p2p_proto::PendingSpend as ProstMessage>::decode(data)
            .map_err(|e| format!("Failed to decode withdrawal intent: {}", e))?;

    let tx = bitcoin::consensus::encode::deserialize(&proto_intent.transaction)
        .map_err(|e| format!("Failed to deserialize transaction: {}", e))?;

    let recipient_script = bitcoin::ScriptBuf::from_bytes(proto_intent.recipient_script);

    Ok(PendingSpend {
        tx,
        user_pubkey: proto_intent.user_pubkey,
        recipient_script,
        fee: proto_intent.fee,
    })
}

#[async_trait::async_trait]
impl<N: Network, D: Db, W: Wallet> Handler<N, D, W> for SpendIntentState {
    async fn handle(
        &mut self,
        node: &mut NodeState<N, D, W>,
        message: Option<NetworkEvent>,
    ) -> Result<(), NodeError> {
        match message {
            Some(NetworkEvent::SelfRequest {
                request: SelfRequest::ProposeWithdrawal { withdrawal_intent },
                response_channel,
            }) => {
                let (total_amount, challenge) =
                    self.propose_withdrawal(node, &withdrawal_intent).await?;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::ProposeWithdrawalResponse {
                            quote_satoshis: total_amount,
                            challenge,
                        })
                        .map_err(|e| NodeError::Error(e.to_string()))?;
                }
            }
            Some(NetworkEvent::SelfRequest {
                request:
                    SelfRequest::ConfirmWithdrawal {
                        challenge,
                        signature,
                    },
                response_channel,
            }) => {
                self.confirm_withdrawal(node, &challenge, &signature)
                    .await?;
                if let Some(response_channel) = response_channel {
                    response_channel
                        .send(SelfResponse::ConfirmWithdrawalResponse { success: true })
                        .map_err(|e| NodeError::Error(e.to_string()))?;
                }
            }
            Some(NetworkEvent::GossipsubMessage(Message { data, topic, .. })) => {
                if topic.as_str() == "withdrawls" {
                    let spend_intent: PendingSpend = decode_withdrawal_intent(&data)
                        .map_err(|e| NodeError::Error(e.to_string()))?;
                    self.handle_withdrawl_message(node, spend_intent).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
