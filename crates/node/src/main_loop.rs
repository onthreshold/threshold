use tracing::info;

use crate::db::Db;
use crate::swarm_manager::{NetworkEvent, SelfRequest, SelfResponse};
use crate::{Network, NodeState};
use types::errors::NodeError;

impl<N: Network, D: Db> NodeState<N, D> {
    pub async fn try_poll(&mut self) -> Result<bool, NodeError> {
        let send_message = self.network_events_stream.try_recv().ok();
        if let Some(event) = send_message {
            self.handle(Some(event)).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn poll(&mut self) -> Result<(), NodeError> {
        let send_message = self.network_events_stream.recv().await;
        self.handle(send_message).await
    }

    pub async fn start(&mut self) -> Result<(), NodeError> {
        info!("Local peer id: {}", self.peer_id);

        loop {
            self.poll().await?
        }
    }

    pub async fn handle(&mut self, send_message: Option<NetworkEvent>) -> Result<(), NodeError> {
        let mut handlers = std::mem::take(&mut self.handlers);

        for handler in handlers.iter_mut() {
            let handler_message = send_message.as_ref().map(|event| event.into());
            handler.handle(self, handler_message).await?;
        }

        self.handlers = handlers;

        match send_message {
            Some(NetworkEvent::SelfRequest {
                request,
                response_channel,
            }) => match request {
                SelfRequest::GetFrostPublicKey => {
                    let response = self.get_frost_public_key();
                    if let Some(response_channel) = response_channel {
                        response_channel
                            .send(SelfResponse::GetFrostPublicKeyResponse {
                                public_key: response,
                            })
                            .map_err(|e| {
                                NodeError::Error(format!("Failed to send response: {}", e))
                            })?;
                    }
                }
                _ => {}
            },
            Some(NetworkEvent::PeersConnected(list)) => {
                for (peer_id, _multiaddr) in list {
                    self.peers.insert(peer_id);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
