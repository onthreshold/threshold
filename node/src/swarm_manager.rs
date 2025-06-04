use libp2p::{PeerId, request_response::ResponseChannel};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

use frost_secp256k1::keys::dkg::round2;
use libp2p::{
    StreamProtocol, Swarm, gossipsub, mdns, noise, request_response, swarm::NetworkBehaviour, tcp,
    yamux,
};
use libp2p::{identity::Keypair, request_response::cbor};
use tokio::{
    io,
    sync::mpsc::{self, unbounded_channel},
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PingBody {
    pub message: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum PrivateRequest {
    Ping(PingBody),
    Round2Package(round2::Package),
    SignRequest { sign_id: u64, message: Vec<u8> },
    SignPackage { sign_id: u64, package: Vec<u8> },

    StartSigningSession { hex_message: String },
    Spend { amount_sat: u64 },
    GetFrostPublicKey,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum PrivateResponse {
    Pong,
    Commitments {
        sign_id: u64,
        commitments: Vec<u8>,
    },
    SignatureShare {
        sign_id: u64,
        signature_share: Vec<u8>,
    },

    StartSigningSession {
        sign_id: u64,
    },
    SpendRequestSent {
        sighash: String,
    },
    GetFrostPublicKey {
        public_key: String,
    },
}

#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub request_response: cbor::Behaviour<PrivateRequest, PrivateResponse>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct NodeError {
    pub message: String,
}

pub enum NetworkMessage {
    SendBroadcast {
        topic: gossipsub::IdentTopic,
        message: Vec<u8>,
    },
    SendPrivateRequest(PeerId, PrivateRequest),
    SendPrivateResponse(ResponseChannel<PrivateResponse>, PrivateResponse),

    SendSelfRequest {
        request: PrivateRequest,
        response_channel: mpsc::UnboundedSender<PrivateResponse>,
    },
}

#[derive(Debug, Clone)]
pub struct NetworkHandle {
    peer_id: PeerId,
    tx: mpsc::UnboundedSender<NetworkMessage>,
}

impl NetworkHandle {
    pub fn send_broadcast(&self, topic: gossipsub::IdentTopic, message: Vec<u8>) {
        let network_message = NetworkMessage::SendBroadcast { topic, message };
        self.tx.send(network_message).unwrap();
    }

    pub fn send_private_request(&self, peer_id: PeerId, request: PrivateRequest) {
        let network_message = NetworkMessage::SendPrivateRequest(peer_id, request);
        self.tx.send(network_message).unwrap();
    }

    pub fn send_private_response(
        &self,
        channel: ResponseChannel<PrivateResponse>,
        response: PrivateResponse,
    ) {
        let network_message = NetworkMessage::SendPrivateResponse(channel, response);
        self.tx.send(network_message).unwrap();
    }

    pub async fn send_self_request(&self, request: PrivateRequest) -> Option<PrivateResponse> {
        let (tx, mut rx) = unbounded_channel::<PrivateResponse>();
        let network_message = NetworkMessage::SendSelfRequest {
            request,
            response_channel: tx,
        };
        self.tx.send(network_message).unwrap();

        rx.recv().await
    }
}

pub struct SwarmManager {
    pub inner: Swarm<MyBehaviour>,

    pub rx: mpsc::UnboundedReceiver<NetworkMessage>,
}

pub fn build_swarm(keypair: Keypair) -> Result<(NetworkHandle, SwarmManager), NodeError> {
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|e| NodeError {
            message: format!("Failed to add tcp {}", e),
        })?
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
        .map_err(|e| NodeError {
            message: format!("Failed to add behaviour {}", e),
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm
        .listen_on(
            "/ip4/0.0.0.0/udp/0/quic-v1"
                .parse()
                .expect("Failed to deserialize message"),
        )
        .map_err(|e| NodeError {
            message: format!("Failed to listen on quic {}", e),
        })?;

    swarm
        .listen_on(
            "/ip4/0.0.0.0/tcp/0"
                .parse()
                .expect("Failed to deserialize message"),
        )
        .map_err(|e| NodeError {
            message: format!("Failed to listen on tcp {}", e),
        })?;

    let (outgoing_tx, incoming_rx) = unbounded_channel::<NetworkMessage>();

    let network = NetworkHandle {
        tx: outgoing_tx.clone(),
        peer_id: *swarm.local_peer_id(),
    };

    Ok((
        network,
        SwarmManager {
            inner: swarm,
            rx: incoming_rx,
        },
    ))
}