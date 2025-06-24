use futures::StreamExt;
use libp2p::{
    PeerId,
    request_response::{Event, Message},
    swarm::SwarmEvent,
};
use std::{
    collections::{BTreeMap, HashSet, hash_map::DefaultHasher},
    fmt::Debug,
    hash::{Hash, Hasher},
    time::Duration,
};
use tracing::info;

// Include the generated P2P proto code

use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::identity::Keypair;
use libp2p::{
    StreamProtocol, Swarm, gossipsub, mdns, noise, request_response, swarm::NetworkBehaviour, tcp,
    yamux,
};
use prost::Message as ProstMessage;
use protocol::transaction::Transaction;
use tokio::{
    io::{self},
    sync::{
        broadcast,
        mpsc::{self, unbounded_channel},
    },
};

use crate::PeerData;
use types::{
    broadcast_received_metrics, broadcast_sent_metrics,
    errors::{NetworkError, NodeError},
    network::network_protocol::{NetworkHandle, NetworkMessage, NetworkResponseFuture},
    proto::p2p_proto,
};
use types::{
    network::network_event::{DirectMessage, NetworkEvent, SelfRequest, SelfResponse},
    proto::{ProtoDecode, ProtoEncode},
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ConsensusMessage {
    Propose(Transaction),
    Prevote(Transaction),
    Precommit(Transaction),
}

#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub request_response: request_response::Behaviour<ProtobufCodec>,
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
        if let Ok(broadcast_msg) = types::broadcast::BroadcastMessage::decode(
            &message.encode().map_err(NetworkError::SendError)?,
        ) {
            broadcast_sent_metrics!(get_broadcast_message_type(&broadcast_msg));
        }

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
            let (tx, mut rx) = unbounded_channel::<SelfResponse>();

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

pub struct SwarmManager {
    pub inner: Swarm<MyBehaviour>,

    pub network_manager_rx: mpsc::UnboundedReceiver<NetworkMessage>,
    pub network_events: broadcast::Sender<NetworkEvent>,

    pub allowed_peers: Vec<PeerId>,
    pub peers_to_names: BTreeMap<PeerId, String>,

    pub live_peers: HashSet<PeerId>,

    pub broadcast_topic: gossipsub::IdentTopic,
}

impl SwarmManager {
    pub fn new(
        mut swarm: Swarm<MyBehaviour>,
        peer_data: &[PeerData],
    ) -> Result<(Self, NetworkHandle), NodeError> {
        let (send_commands, receiving_commands) = unbounded_channel::<NetworkMessage>();

        let (network_events_emitter, _) = broadcast::channel::<NetworkEvent>(200);

        let broadcast_topic = gossipsub::IdentTopic::new("broadcast");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&broadcast_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let allowed_peers: Vec<PeerId> = peer_data
            .iter()
            .map(|peer| peer.public_key.parse().unwrap())
            .collect();

        let peers_to_names: BTreeMap<PeerId, String> = peer_data
            .iter()
            .map(|peer| (peer.public_key.parse().unwrap(), peer.name.clone()))
            .collect();

        let network_handle = NetworkHandle {
            peer_id: *swarm.local_peer_id(),
            tx: send_commands,
            peers_to_names: peers_to_names.clone(),
        };

        Ok((
            Self {
                broadcast_topic,
                inner: swarm,
                network_manager_rx: receiving_commands,
                network_events: network_events_emitter,
                allowed_peers,
                peers_to_names,
                live_peers: HashSet::new(),
            },
            network_handle,
        ))
    }

    pub fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .map_or_else(|| peer_id.to_string(), Clone::clone)
    }

    pub async fn start(&mut self) {
        info!("Starting swarm manager");
        loop {
            tokio::select! {
                send_message = self.network_manager_rx.recv() => match send_message {
                    Some(NetworkMessage::SendBroadcast { topic, message }) => {
                        let _ = self.inner
                            .behaviour_mut()
                            .gossipsub
                            .publish(topic, message);
                    }
                    Some(NetworkMessage::SendPrivateMessage(peer_id, request)) => {
                        self.inner
                            .behaviour_mut()
                            .request_response
                            .send_request(&peer_id, request);
                    }
                    Some(NetworkMessage::SendSelfRequest { request, response_channel }) => {
                        self.network_events.send(NetworkEvent::SelfRequest { request, response_channel } ).unwrap();
                    }
                    _ => {
                    }
                },
                event = self.inner.select_next_some() => {
                    match event {
                        SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                            let mut peers_connected = vec![];
                            for (peer_id, multiaddr) in list {
                                if self.allowed_peers.contains(&peer_id) {
                                    info!("Discovered peer: {}", self.peer_name(&peer_id));
                                    peers_connected.push((peer_id, multiaddr));
                                    self.live_peers.insert(peer_id);
                                    self.inner.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                }
                            }
                            self.network_events.send(NetworkEvent::PeersConnected(peers_connected)).unwrap();
                        },
                        SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                            for (peer_id, _multiaddr) in list.clone() {
                                if self.allowed_peers.contains(&peer_id) {
                                    info!("Peer expired: {}", self.peer_name(&peer_id));
                                    self.live_peers.retain(|p| p != &peer_id);
                                    self.inner.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                }
                            }
                            self.network_events.send(NetworkEvent::PeersDisconnected(list)).unwrap();
                        },
                        SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                            message,
                            ..
                        })) => {
                            if let Ok(broadcast_msg) = types::broadcast::BroadcastMessage::decode(&message.data) {
                                broadcast_received_metrics!(get_broadcast_message_type(&broadcast_msg));
                            }
                            self.network_events.send(NetworkEvent::GossipsubMessage(message.clone())).unwrap();
                        },
                        SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(Event::Message {
                            peer,
                            message: Message::Request { request, .. },
                            ..
                        }) ) => {
                            self.network_events.send(NetworkEvent::MessageEvent((peer, request))).unwrap();
                        },
                        SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic })) => {
                            self.network_events.send(NetworkEvent::Subscribed { peer_id, topic }).unwrap();
                        },
                        _ => {
                            // self.network_events.send(NetworkEvent::SwarmEvent(event)).unwrap();
                        }
                    }
                }

            }
        }
    }
}

pub fn build_swarm(
    keypair: Keypair,
    libp2p_udp_port: u16,
    libp2p_tcp_port: u16,
    peer_data: &[PeerData],
) -> Result<(NetworkHandle, SwarmManager), NodeError> {
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|e| NodeError::Error(format!("Failed to add tcp {e}")))?
        .with_quic()
        .with_behaviour(|key| {
            // To content-address message, we can take the hash of message and use it as an ID.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            let mesh_high_env: usize = std::env::var("GOSSIPSUB_MESH_HIGH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| std::cmp::max(12, peer_data.len().saturating_sub(1)));

            let mesh_n_high = mesh_high_env.clamp(4, 512);

            let mesh_n = std::cmp::max(3, mesh_n_high * 2 / 3);
            let mesh_n_low = std::cmp::max(1, mesh_n / 2);

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(5))
                .validation_mode(gossipsub::ValidationMode::Strict)
                .message_id_fn(message_id_fn)
                .mesh_n_low(mesh_n_low)
                .mesh_n_high(mesh_n_high)
                .mesh_n(mesh_n)
                .mesh_outbound_min(mesh_n_low)
                .gossip_lazy(std::cmp::max(3, mesh_n_low))
                .max_transmit_size(64 * 1024)
                .flood_publish(true)
                .build()
                .map_err(io::Error::other)?;

            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;

            let request_response = request_response::Behaviour::with_codec(
                ProtobufCodec,
                [(
                    StreamProtocol::new("/direct-message/1.0.0"),
                    request_response::ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            );

            Ok(MyBehaviour {
                gossipsub,
                mdns,
                request_response,
            })
        })
        .map_err(|e| NodeError::Error(format!("Failed to add behaviour {e}")))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm
        .listen_on(
            format!("/ip4/0.0.0.0/udp/{libp2p_udp_port}/quic-v1")
                .parse()
                .expect("Failed to deserialize message"),
        )
        .map_err(|e| NodeError::Error(format!("Failed to listen on quic {e}")))?;

    swarm
        .listen_on(
            format!("/ip4/0.0.0.0/tcp/{libp2p_tcp_port}")
                .parse()
                .expect("Failed to deserialize message"),
        )
        .map_err(|e| NodeError::Error(format!("Failed to listen on tcp {e}")))?;

    let (swarm_manager, network) = SwarmManager::new(swarm, peer_data)
        .map_err(|e| NodeError::Error(format!("Failed to create swarm manager: {e}")))?;

    Ok((network, swarm_manager))
}

// Custom protobuf codec for request-response
#[derive(Debug, Clone)]
pub struct ProtobufCodec;

#[async_trait::async_trait]
impl libp2p::request_response::Codec for ProtobufCodec {
    type Protocol = libp2p::StreamProtocol;
    type Request = types::network::network_event::DirectMessage;
    type Response = ();

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        // Read the protobuf message
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;

        let proto_msg =
            <p2p_proto::DirectMessage as ProstMessage>::decode(&buf[..]).map_err(|e| {
                tracing::error!("❌ Failed to decode protobuf DirectMessage: {}", e);
                std::io::Error::new(std::io::ErrorKind::InvalidData, e)
            })?;

        let direct_msg = types::network::network_event::DirectMessage::try_from(proto_msg)
            .map_err(|e| {
                tracing::error!("❌ Failed to convert protobuf to DirectMessage: {}", e);
                std::io::Error::new(std::io::ErrorKind::InvalidData, e)
            })?;

        Ok(direct_msg)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        _io: &mut T,
    ) -> std::io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        // We don't use responses for direct messages
        Ok(())
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Convert to protobuf
        let proto_msg = p2p_proto::DirectMessage::from(req);

        // Encode protobuf
        let mut buf = Vec::new();
        <p2p_proto::DirectMessage as ProstMessage>::encode(&proto_msg, &mut buf).map_err(|e| {
            tracing::error!("❌ Failed to encode DirectMessage to protobuf: {}", e);
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;

        let len = buf.len();

        io.write_all(&u32::try_from(len).unwrap().to_be_bytes())
            .await?;

        // Write the message
        io.write_all(&buf).await?;
        io.flush().await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        _io: &mut T,
        _res: Self::Response,
    ) -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // We don't use responses for direct messages
        Ok(())
    }
}

const fn get_broadcast_message_type(
    broadcast_msg: &types::broadcast::BroadcastMessage,
) -> &'static str {
    match broadcast_msg {
        types::broadcast::BroadcastMessage::Consensus(_) => "consensus",
        types::broadcast::BroadcastMessage::Block(_) => "block",
        types::broadcast::BroadcastMessage::DepositIntent(_) => "deposit_intent",
        types::broadcast::BroadcastMessage::PendingSpend(_) => "pending_spend",
        types::broadcast::BroadcastMessage::Dkg(_) => "dkg",
    }
}
