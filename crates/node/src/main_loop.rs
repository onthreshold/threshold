use tracing::{error, info};

use crate::wallet::Wallet;
use crate::{Network, NodeState};
use types::errors::NodeError;
use types::network::network_event::NetworkEvent;

impl<N: Network + 'static, W: Wallet + 'static> NodeState<N, W> {
    pub async fn try_poll(&mut self) -> Result<bool, NodeError> {
        let send_message = self.network_events_stream.try_recv().ok();
        if let Some(event) = send_message {
            self.handle_message(event).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn poll(&mut self) -> Result<(), NodeError> {
        if let Ok(event) = self.network_events_stream.recv().await {
            self.handle_message(event).await?;
        }
        Ok(())
    }

    pub async fn start(&mut self) {
        info!("Local peer id: {}", self.peer_id);
        loop {
            if let Err(e) = self.poll().await {
                error!("Error polling network events: {}", e);
            }
        }
    }

    pub async fn handle_message(&mut self, message: NetworkEvent) -> Result<(), NodeError> {
        let mut handlers = std::mem::take(&mut self.handlers);

        for handler in &mut handlers {
            handler.handle(self, message.clone()).await?;
        }

        self.handlers = handlers;

        match message {
            NetworkEvent::PeersConnected(list) => {
                for (peer_id, _multiaddr) in list {
                    self.peers.insert(peer_id);
                }
            }
            NetworkEvent::SendBroadcast { message } => {
                // Forward broadcast request to the network handle
                if let Err(e) = self.network_handle.send_broadcast(message) {
                    error!("Failed to send broadcast: {:?}", e);
                }
            }
            _ => {}
        }

        Ok(())
    }
}
