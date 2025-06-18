use tokio::sync::broadcast;
use types::errors::NodeError;

pub struct Sender<M, R> {
    pub(crate) tx: broadcast::Sender<(M, broadcast::Sender<R>)>,
    pub(crate) reverse_tx: broadcast::Sender<R>,
    pub(crate) reverse_rx: broadcast::Receiver<R>,
}

pub type Reciver<M, R> = broadcast::Receiver<(M, broadcast::Sender<R>)>;

#[must_use]
pub fn channel<M: Clone, R: Clone>(
    buffer: usize,
    reverse_buffer: Option<usize>,
) -> (Sender<M, R>, Reciver<M, R>) {
    let (tx, rx) = broadcast::channel(buffer);
    let (reverse_tx, reverse_rx) = broadcast::channel(reverse_buffer.unwrap_or(buffer));
    (
        Sender {
            tx,
            reverse_tx,
            reverse_rx,
        },
        rx,
    )
}

impl<M, R> Sender<M, R>
where
    M: Clone,
    R: Clone,
{
    pub fn send_message(&self, message: M) -> Result<(), NodeError> {
        self.tx
            .send((message, self.reverse_tx.clone()))
            .map_err(|e| NodeError::Error(format!("Failed to send message: {e}")))?;
        Ok(())
    }

    pub async fn send_message_with_response(&mut self, message: M) -> Result<R, NodeError> {
        self.send_message(message)?;
        let response = self
            .reverse_rx
            .recv()
            .await
            .map_err(|e| NodeError::Error(format!("Failed to receive response: {e}")))?;
        Ok(response)
    }
}
