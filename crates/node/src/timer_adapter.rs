use std::time::Duration;

use tokio::task::JoinHandle;

use crate::{NodeState, wallet::Wallet};
use round_timer::start_round_timer;
use types::network::network_protocol::Network;

pub trait RoundTimerControl {
    fn launch_round_timer(&self, interval: Duration) -> JoinHandle<()>;
}

impl<N: Network + 'static, W: Wallet> RoundTimerControl for NodeState<N, W> {
    fn launch_round_timer(&self, interval: Duration) -> JoinHandle<()> {
        let sender = self.network_events_sender.clone();
        start_round_timer(sender, interval)
    }
}

