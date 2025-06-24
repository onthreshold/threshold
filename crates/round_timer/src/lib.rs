use std::time::Duration;

use tokio::{runtime::Handle, sync::broadcast, task::JoinHandle, time::interval};
use tracing::error;

use types::network::network_event::{NetworkEvent, SelfRequest};

pub fn start_round_timer(
    sender: broadcast::Sender<NetworkEvent>,
    tick_interval: Duration,
) -> JoinHandle<()> {
    Handle::current().spawn(async move {
        let mut ticker = interval(tick_interval);
        loop {
            ticker.tick().await;
            if let Err(broadcast::error::SendError(_)) = sender.send(NetworkEvent::SelfRequest {
                request: SelfRequest::Tick,
                response_channel: None,
            }) {
                error!("Round timer tick failing. No receivers alive");
            }
        }
    })
}

