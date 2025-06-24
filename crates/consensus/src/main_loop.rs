use crate::{ConsensusInterface, ConsensusInterfaceImpl, ConsensusMessage, ConsensusResponse};
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info};
use types::errors::NodeError;

const POLL_INTERVAL_MS: u64 = 100;
const ROUND_TIME_SECONDS: u64 = 10;

impl ConsensusInterfaceImpl {
    pub async fn start(&mut self) {
        info!(
            "Starting consensus interface main loop with {}ms polling and {}s rounds",
            POLL_INTERVAL_MS, ROUND_TIME_SECONDS
        );

        let mut poll_interval = interval(Duration::from_millis(POLL_INTERVAL_MS));
        let mut round_interval = interval(Duration::from_secs(ROUND_TIME_SECONDS));

        // Skip the first tick to avoid immediate firing
        poll_interval.tick().await;
        round_interval.tick().await;

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    if let Err(e) = self.poll_messages().await {
                        error!("Error polling consensus messages: {}", e);
                    }
                }
                _ = round_interval.tick() => {
                    if let Err(e) = self.trigger_new_round().await {
                        error!("Error triggering new consensus round: {}", e);
                    }
                }
            }
        }
    }

    async fn poll_messages(&mut self) -> Result<(), NodeError> {
        // Try to receive messages without blocking
        while let Ok((message, response_sender)) = self.message_stream.try_recv() {
            let response = self.handle_message(message).await;
            if let Err(e) = response_sender.send(response) {
                error!("Failed to send consensus response: {}", e);
            }
        }
        Ok(())
    }

    async fn trigger_new_round(&mut self) -> Result<(), NodeError> {
        // Only trigger new rounds if we have validators and consensus is active
        if self.state.validators.len() >= 2 && self.state.current_round > 0 {
            debug!(
                "Auto-triggering new consensus round after {}s interval",
                ROUND_TIME_SECONDS
            );
            self.start_new_round()?;

            // If we're the leader, automatically propose a block
            if self.state.is_leader {
                if let Err(e) = self.propose_block_as_leader().await {
                    error!("Failed to propose block as leader: {}", e);
                }
            }
        }
        Ok(())
    }

    pub async fn poll(&mut self) -> Result<(), NodeError> {
        match self.message_stream.recv().await {
            Ok((message, response_sender)) => {
                let response = self.handle_message(message).await;
                if let Err(e) = response_sender.send(response) {
                    error!("Failed to send consensus response: {}", e);
                }
                Ok(())
            }
            Err(e) => Err(NodeError::Error(format!(
                "Failed to receive consensus message: {e}"
            ))),
        }
    }

    pub async fn handle(
        &mut self,
        message: Option<(
            ConsensusMessage,
            tokio::sync::broadcast::Sender<ConsensusResponse>,
        )>,
    ) -> Result<(), NodeError> {
        if let Some((msg, response_sender)) = message {
            let response = self.handle_message(msg).await;
            if let Err(e) = response_sender.send(response) {
                error!("Failed to send consensus response: {}", e);
            }
        }
        Ok(())
    }
}
