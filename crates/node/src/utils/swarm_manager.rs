use futures::StreamExt;
use libp2p::{
    PeerId,
    request_response::{Event, Message},
    swarm::SwarmEvent,
};
use std::{
    collections::{BTreeMap, HashSet, hash_map::DefaultHasher},
    fmt::Debug,
    future::Future,
    hash::{Hash, Hasher},
    pin::Pin,
    time::Duration,
};
use tracing::info;

// Include the generated P2P proto code
pub mod p2p_proto {
    tonic::include_proto!("p2p");
}

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
use types::errors::{NetworkError, NodeError};
use types::network_event::{DirectMessage, NetworkEvent, SelfRequest, SelfResponse};

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

pub type NetworkResponseFuture =
    Pin<Box<dyn Future<Output = Result<SelfResponse, NetworkError>> + Send>>;

#[derive(Debug, Clone)]
pub struct NetworkHandle {
    peer_id: PeerId,
    tx: mpsc::UnboundedSender<NetworkMessage>,
}

pub trait Network: Clone + Debug + Sync + Send {
    fn peer_id(&self) -> PeerId;
    fn send_broadcast(
        &self,
        topic: gossipsub::IdentTopic,
        message: Vec<u8>,
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
}

impl Network for NetworkHandle {
    fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    fn send_broadcast(
        &self,
        topic: gossipsub::IdentTopic,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        let network_message = NetworkMessage::SendBroadcast { topic, message };
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
        self.tx
            .send(network_message)
            .map_err(|e| NetworkError::SendError(e.to_string()))
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
}

pub struct SwarmManager {
    pub inner: Swarm<MyBehaviour>,

    pub network_manager_rx: mpsc::UnboundedReceiver<NetworkMessage>,
    pub network_events: broadcast::Sender<NetworkEvent>,

    pub allowed_peers: Vec<PeerId>,
    pub peers_to_names: BTreeMap<PeerId, String>,

    pub live_peers: HashSet<PeerId>,

    pub round1_topic: gossipsub::IdentTopic,
    pub start_dkg_topic: gossipsub::IdentTopic,
    pub deposit_intents_topic: gossipsub::IdentTopic,
    pub withdrawls_topic: gossipsub::IdentTopic,
    pub leader_topic: gossipsub::IdentTopic,
}

impl SwarmManager {
    pub fn new(
        mut swarm: Swarm<MyBehaviour>,
        peer_data: Vec<PeerData>,
    ) -> Result<(Self, NetworkHandle), NodeError> {
        let (send_commands, receiving_commands) = unbounded_channel::<NetworkMessage>();

        let (network_events_emitter, _) = broadcast::channel::<NetworkEvent>(100);

        let network_handle = NetworkHandle {
            peer_id: *swarm.local_peer_id(),
            tx: send_commands,
        };

        // Read full lines from stdin
        let round1_topic = gossipsub::IdentTopic::new("round1_topic");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&round1_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let allowed_peers: Vec<PeerId> = peer_data
            .iter()
            .map(|peer| peer.public_key.parse().unwrap())
            .collect();

        let peers_to_names: BTreeMap<PeerId, String> = peer_data
            .iter()
            .map(|peer| (peer.public_key.parse().unwrap(), peer.name.clone()))
            .collect();

        let start_dkg_topic = gossipsub::IdentTopic::new("start-dkg");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&start_dkg_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let deposit_intents_topic = gossipsub::IdentTopic::new("deposit-intents");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&deposit_intents_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let withdrawls_topic = gossipsub::IdentTopic::new("withdrawls");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&withdrawls_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        let leader_topic = gossipsub::IdentTopic::new("leader");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&leader_topic)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        Ok((
            Self {
                round1_topic,
                live_peers: HashSet::new(),
                start_dkg_topic,
                deposit_intents_topic,
                withdrawls_topic,
                leader_topic,
                inner: swarm,
                network_manager_rx: receiving_commands,
                network_events: network_events_emitter,
                allowed_peers,
                peers_to_names,
            },
            network_handle,
        ))
    }

    pub fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .unwrap_or(&peer_id.to_string())
            .clone()
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
    peer_data: Vec<PeerData>,
) -> Result<(NetworkHandle, SwarmManager), NodeError> {
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|e| NodeError::Error(format!("Failed to add tcp {}", e)))?
        .with_quic()
        .with_behaviour(|key| {
            // To content-address message, we can take the hash of message and use it as an ID.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
                .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
                .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                .mesh_n_low(1) // Minimum number of peers in mesh network (default is 4)
                .mesh_n_high(12) // Maximum number of peers in mesh network
                .mesh_n(3) // Target number of peers in mesh network (default is 6)
                .mesh_outbound_min(1) // Minimum outbound connections (default is 2)
                .gossip_lazy(3) // Number of peers to gossip to (default is 6)
                .flood_publish(true) // Always flood publish messages to all peers, regardless of mesh
                .build()
                .map_err(io::Error::other)?; // Temporary hack because `build` does not return a proper `std::error::Error`.

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
        .map_err(|e| NodeError::Error(format!("Failed to add behaviour {}", e)))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm
        .listen_on(
            format!("/ip4/0.0.0.0/udp/{}/quic-v1", libp2p_udp_port)
                .parse()
                .expect("Failed to deserialize message"),
        )
        .map_err(|e| NodeError::Error(format!("Failed to listen on quic {}", e)))?;

    swarm
        .listen_on(
            format!("/ip4/0.0.0.0/tcp/{}", libp2p_tcp_port)
                .parse()
                .expect("Failed to deserialize message"),
        )
        .map_err(|e| NodeError::Error(format!("Failed to listen on tcp {}", e)))?;

    let (swarm_manager, network) = SwarmManager::new(swarm, peer_data)
        .map_err(|e| NodeError::Error(format!("Failed to create swarm manager: {}", e)))?;

    Ok((network, swarm_manager))
}

// Conversion functions between DirectMessage and protobuf
impl From<types::network_event::DirectMessage> for p2p_proto::DirectMessage {
    fn from(msg: types::network_event::DirectMessage) -> Self {
        use p2p_proto::direct_message::Message;
        use types::network_event::DirectMessage::*;

        let message = match msg {
            Ping(ping_body) => Message::Ping(p2p_proto::PingMessage {
                message: ping_body.message,
            }),
            Pong => Message::Pong(p2p_proto::PongMessage {}),
            Round2Package(package) => {
                let serialized =
                    serde_json::to_vec(&package).expect("Failed to serialize round2 package");
                Message::Round2Package(p2p_proto::Round2Package {
                    package_data: serialized,
                })
            }
            SignRequest { sign_id, message } => {
                Message::SignRequest(p2p_proto::SignRequest { sign_id, message })
            }
            SignPackage { sign_id, package } => {
                Message::SignPackage(p2p_proto::SignPackage { sign_id, package })
            }
            Commitments {
                sign_id,
                commitments,
            } => Message::Commitments(p2p_proto::Commitments {
                sign_id,
                commitments,
            }),
            SignatureShare {
                sign_id,
                signature_share,
            } => Message::SignatureShare(p2p_proto::SignatureShare {
                sign_id,
                signature_share,
            }),
        };

        p2p_proto::DirectMessage {
            message: Some(message),
        }
    }
}

impl TryFrom<p2p_proto::DirectMessage> for types::network_event::DirectMessage {
    type Error = String;

    fn try_from(proto_msg: p2p_proto::DirectMessage) -> Result<Self, Self::Error> {
        use p2p_proto::direct_message::Message;
        use types::network_event::{DirectMessage, PingBody};

        let message = proto_msg.message.ok_or("Missing message field")?;

        match message {
            Message::Ping(ping) => Ok(DirectMessage::Ping(PingBody {
                message: ping.message,
            })),
            Message::Pong(_) => Ok(DirectMessage::Pong),
            Message::Round2Package(package) => {
                let round2_package = serde_json::from_slice(&package.package_data)
                    .map_err(|e| format!("Failed to deserialize round2 package: {}", e))?;
                Ok(DirectMessage::Round2Package(round2_package))
            }
            Message::SignRequest(req) => Ok(DirectMessage::SignRequest {
                sign_id: req.sign_id,
                message: req.message,
            }),
            Message::SignPackage(pkg) => Ok(DirectMessage::SignPackage {
                sign_id: pkg.sign_id,
                package: pkg.package,
            }),
            Message::Commitments(comm) => Ok(DirectMessage::Commitments {
                sign_id: comm.sign_id,
                commitments: comm.commitments,
            }),
            Message::SignatureShare(share) => Ok(DirectMessage::SignatureShare {
                sign_id: share.sign_id,
                signature_share: share.signature_share,
            }),
        }
    }
}

// Custom protobuf codec for request-response
#[derive(Debug, Clone)]
pub struct ProtobufCodec;

#[async_trait::async_trait]
impl libp2p::request_response::Codec for ProtobufCodec {
    type Protocol = libp2p::StreamProtocol;
    type Request = types::network_event::DirectMessage;
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

        // Decode protobuf
        let proto_msg = <p2p_proto::DirectMessage as ProstMessage>::decode(&buf[..])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Convert to DirectMessage
        let direct_msg = types::network_event::DirectMessage::try_from(proto_msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

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
        <p2p_proto::DirectMessage as ProstMessage>::encode(&proto_msg, &mut buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Write length prefix
        let len = buf.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;

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
