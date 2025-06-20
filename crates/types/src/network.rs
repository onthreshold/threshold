use futures::future::Future;
use libp2p::{gossipsub, PeerId};
use std::{fmt::Debug, pin::Pin};
use tokio::sync::mpsc;

use crate::{
    errors::NetworkError,
    network_event::{DirectMessage, SelfRequest, SelfResponse},
    proto::ProtoEncode,
};

pub type NetworkResponseFuture =
    Pin<Box<dyn Future<Output = Result<SelfResponse, NetworkError>> + Send>>;

#[derive(Debug, Clone)]
pub struct NetworkHandle {
    pub peer_id: PeerId,
    pub tx: mpsc::UnboundedSender<NetworkMessage>,
    pub peers_to_names: std::collections::BTreeMap<PeerId, String>,
}

#[derive(Clone, Debug)]
pub enum NetworkMessage {
    SendBroadcast {
        topic: gossipsub::IdentTopic,
        message: Vec<u8>,
    },
    SendPrivateMessage(PeerId, DirectMessage),
    SendSelfRequest {
        request: SelfRequest,
        response_channel: Option<mpsc::UnboundedSender<SelfResponse>>,
    },
}

pub trait Network: Clone + Debug + Sync + Send {
    fn peer_id(&self) -> PeerId;
    fn send_broadcast(
        &self,
        topic: gossipsub::IdentTopic,
        message: impl ProtoEncode,
    ) -> Result<(), NetworkError>;
    fn send_private_message(
        &self,
        peer_id: PeerId,
        request: DirectMessage,
    ) -> Result<(), NetworkError>;
    fn send_self_request(
        &self,
        request: SelfRequest,
        sync: bool,
    ) -> Result<Option<NetworkResponseFuture>, NetworkError>;
    fn peer_name(&self, peer_id: &PeerId) -> String;
}

impl Network for NetworkHandle {
    fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    fn send_broadcast(
        &self,
        topic: gossipsub::IdentTopic,
        message: impl ProtoEncode,
    ) -> Result<(), NetworkError> {
        let network_message = NetworkMessage::SendBroadcast {
            topic,
            message: message.encode().map_err(NetworkError::SendError)?,
        };
        self.tx
            .send(network_message)
            .map_err(|e| NetworkError::SendError(e.to_string()))
    }

    fn send_private_message(
        &self,
        peer_id: PeerId,
        request: DirectMessage,
    ) -> Result<(), NetworkError> {
        let network_message = NetworkMessage::SendPrivateMessage(peer_id, request);
        self.tx.send(network_message).map_err(|e| {
            tracing::error!("❌ Failed to send private message to {}: {}", peer_id, e);
            NetworkError::SendError(e.to_string())
        })
    }

    fn send_self_request(
        &self,
        request: SelfRequest,
        sync: bool,
    ) -> Result<Option<NetworkResponseFuture>, NetworkError> {
        if sync {
            let (tx, mut rx) = mpsc::unbounded_channel::<SelfResponse>();

            let network_message = NetworkMessage::SendSelfRequest {
                request,
                response_channel: Some(tx),
            };

            self.tx
                .send(network_message)
                .map_err(|e| NetworkError::SendError(e.to_string()))?;

            Ok(Some(Box::pin(async move {
                rx.recv().await.ok_or(NetworkError::RecvError)
            })))
        } else {
            let network_message = NetworkMessage::SendSelfRequest {
                request,
                response_channel: None,
            };

            self.tx
                .send(network_message)
                .map_err(|e| NetworkError::SendError(e.to_string()))?;

            Ok(None)
        }
    }

    fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .map_or_else(|| peer_id.to_string(), Clone::clone)
    }
}

impl NetworkHandle {
    pub fn new(
        peer_id: PeerId,
        tx: mpsc::UnboundedSender<NetworkMessage>,
        peers_to_names: std::collections::BTreeMap<PeerId, String>,
    ) -> Self {
        Self {
            peer_id,
            tx,
            peers_to_names,
        }
    }
} 