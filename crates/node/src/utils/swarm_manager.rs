use futures::StreamExt;
use libp2p::{
    Multiaddr, PeerId,
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

use frost_secp256k1::keys::dkg::round2;
use libp2p::{
    StreamProtocol, Swarm, gossipsub, mdns, noise, request_response, swarm::NetworkBehaviour, tcp,
    yamux,
};
use libp2p::{identity::Keypair, request_response::cbor};
use tokio::{
    io,
    sync::{
        broadcast,
        mpsc::{self, unbounded_channel},
    },
};

use crate::{PeerData, handlers::deposit::DepositIntent, handlers::withdrawl::SpendIntent};
use types::errors::{NetworkError, NodeError};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PingBody {
    pub message: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum DirectMessage {
    Ping(PingBody),
    Round2Package(round2::Package),
    SignRequest {
        sign_id: u64,
        message: Vec<u8>,
    },
    SignPackage {
        sign_id: u64,
        package: Vec<u8>,
    },
    Pong,
    Commitments {
        sign_id: u64,
        commitments: Vec<u8>,
    },
    SignatureShare {
        sign_id: u64,
        signature_share: Vec<u8>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SelfRequest {
    CreateDeposit {
        amount_sat: u64,
    },
    GetPendingDepositIntents,
    StartSigningSession {
        hex_message: String,
    },
    Spend {
        amount_sat: u64,
        address_to: String,
    },
    ProposeWithdrawal {
        withdrawal_intent: SpendIntent,
    },
    ConfirmWithdrawal {
        challenge: String,
        signature: String,
    },
    CheckBalance {
        address: String,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SelfResponse {
    GetFrostPublicKeyResponse {
        public_key: Option<String>,
    },
    CreateDepositResponse {
        deposit_tracking_id: String,
        deposit_address: String,
    },
    GetPendingDepositIntentsResponse {
        intents: Vec<DepositIntent>,
    },
    StartSigningSessionResponse {
        sign_id: u64,
    },
    SpendRequestSent {
        sighash: String,
    },
    ProposeWithdrawalResponse {
        quote_satoshis: u64,
        challenge: String,
    },
    ConfirmWithdrawalResponse {
        success: bool,
    },
    CheckBalanceResponse {
        balance_satoshis: u64,
    },
}

#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub request_response: cbor::Behaviour<DirectMessage, ()>,
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

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    SelfRequest {
        request: SelfRequest,
        response_channel: Option<mpsc::UnboundedSender<SelfResponse>>,
    },
    Subscribed {
        peer_id: PeerId,
        topic: gossipsub::TopicHash,
    },
    GossipsubMessage(gossipsub::Message),
    MessageEvent((PeerId, DirectMessage)),
    PeersConnected(Vec<(PeerId, Multiaddr)>),
    PeersDisconnected(Vec<(PeerId, Multiaddr)>),
    Unknown,
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

        Ok((
            Self {
                round1_topic,
                live_peers: HashSet::new(),
                start_dkg_topic,
                deposit_intents_topic,
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

            let request_response = cbor::Behaviour::new(
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
