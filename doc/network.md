# Networking

## Overview

The networking architecture is built around a P2P communication system using libp2p, designed to support distributed threshold signing and blockchain operations. The system uses an event-driven model where `NetworkEvents` flow into the node for processing, and `NetworkMessage`s are sent out to communicate with peers.

## Core Components

### NodeState

The `NodeState` is the central state management component that owns and coordinates all node operations:

```rust
pub struct NodeState<N: Network, D: Db> {
    pub network_handle: N,
    pub db: D,
    pub handlers: Vec<Box<dyn Handler<N>>>,
    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,
    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,
    pub active_signing: Option<ActiveSigning>,
    pub wallet: crate::wallet::SimpleWallet,
    pub pending_spends: BTreeMap<u64, crate::wallet::PendingSpend>,
    pub config: NodeConfig,
    pub network_events_stream: UnboundedReceiver<NetworkEvent>,
}
```

**What NodeState owns:**
- **Network Interface**: Holds a `Network` implementation handle for sending messages
- **Event Stream**: Receives `NetworkEvents` through `network_events_stream`

### Network Trait

The `Network` trait provides a clean abstraction for network communication:

```rust
pub trait Network: Clone + Debug + Sync {
    fn peer_id(&self) -> PeerId;
    fn send_broadcast(&self, topic: gossipsub::IdentTopic, message: Vec<u8>) -> Result<(), NetworkError>;
    fn send_private_message(&self, peer_id: PeerId, request: DirectMessage) -> Result<(), NetworkError>;
    fn send_self_request(&self, request: SelfRequest, sync: bool) -> Result<Option<NetworkResponseFuture>, NetworkError>;
}
```

**Network Trait Responsibilities:**
- **Peer Identity**: Provides the local peer ID
- **Broadcast Communication**: Send messages to all subscribed peers on a topic
- **Direct Messaging**: Send private messages to specific peers using request-response protocol
- **Self Requests**: Internal message routing for local operations
- **Async Support**: Returns futures for synchronous self-requests

The main implementation is `NetworkHandle`, which acts as a message passing interface to the `SwarmManager`.

### NetworkEvents (Incoming) vs NetworkMessage (Outgoing)

The networking system uses a clear separation between incoming and outgoing message types:

#### NetworkEvents (Received)

```rust
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
    MessageEvent(request_response::Event<DirectMessage, ()>),
    PeersConnected(Vec<(PeerId, Multiaddr)>),
    PeersDisconnected(Vec<(PeerId, Multiaddr)>),
}
```

**NetworkEvents represent:**
- Incoming gossipsub messages from other peers
- Direct messages received via request-response protocol
- Peer connection/disconnection events
- Topic subscription notifications
- Internal self-requests

#### NetworkMessage (Sent)

```rust
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
```

**NetworkMessages represent:**
- Outgoing broadcast messages to gossipsub topics
- Direct messages to specific peers
- Internal routing for self-requests

### SwarmManager

The `SwarmManager` is the networking task that bridges libp2p with the application layer:

```rust
pub struct SwarmManager {
    pub inner: Swarm<MyBehaviour>,
    pub network_manager_rx: mpsc::UnboundedReceiver<NetworkMessage>,
    pub network_events: mpsc::UnboundedSender<NetworkEvent>,
    pub allowed_peers: Vec<PeerId>,
    pub peers_to_names: BTreeMap<PeerId, String>,
    pub live_peers: HashSet<PeerId>,
    pub round1_topic: gossipsub::IdentTopic,
    pub start_dkg_topic: gossipsub::IdentTopic,
}
```

**SwarmManager Responsibilities:**
- **libp2p Integration**: Manages the underlying libp2p swarm
- **Message Translation**: Converts between `NetworkMessage` and libp2p operations
- **Event Processing**: Translates libp2p events to `NetworkEvent`s
- **Peer Filtering**: Only allows communication with configured allowed peers
- **Topic Management**: Handles gossipsub topic subscriptions
- **Connection Management**: Tracks live peer connections

The SwarmManager runs in its own async task and uses a `tokio::select!` loop to handle:
1. Incoming `NetworkMessage`s from the application
2. libp2p swarm events

## Network Event Flow

```mermaid
graph TB
    NS[NodeState] 
    SM[SwarmManager]
    H[Handlers<br/>DKG, Sign]
    NET[P2P Network<br/>Internet]
    
    NS --|NetworkMessage| SM
    SM --|NetworkEvent| NS
    NS --|Handle Events| H
    SM --|libp2p<br/>Swarm Events| NET
    
    subgraph "Application Layer"
        NS
        H
    end
    
    subgraph "Network Layer"
        SM
        NET
    end
    
    style NS fill:#e1f5fe
    style SM fill:#f3e5f5
    style H fill:#e8f5e8
    style NET fill:#fff3e0
```

### Event Flow Details

**Outbound Flow (NodeState → Network):**
1. `NodeState` calls methods on `Network` trait
2. `NetworkHandle` converts calls to `NetworkMessage`
3. `SwarmManager` receives `NetworkMessage` via channel
4. `SwarmManager` translates to appropriate libp2p operations
5. libp2p sends data over the network

**Inbound Flow (Network → NodeState):**
1. libp2p receives network data and generates swarm events
2. `SwarmManager` processes swarm events
3. `SwarmManager` creates appropriate `NetworkEvent`
4. `NetworkEvent` sent to `NodeState` via channel
5. `NodeState` processes event and routes to appropriate handlers

### Message Types

#### DirectMessage (Peer-to-Peer)

```rust
pub enum DirectMessage {
    Ping(PingBody),
    Round2Package(round2::Package),
    SignRequest { sign_id: u64, message: Vec<u8> },
    SignPackage { sign_id: u64, package: Vec<u8> },
    Pong,
    Commitments { sign_id: u64, commitments: Vec<u8> },
    SignatureShare { sign_id: u64, signature_share: Vec<u8> },
}
```

#### SelfRequest (Internal)

```rust
pub enum SelfRequest {
    GetFrostPublicKey,
    StartSigningSession { hex_message: String },
    InsertBlock { block: Block },
    Spend { amount_sat: u64 },
    SetFrostKeys { private_key: Vec<u8>, public_key: Vec<u8> },
}
```

This architecture provides a robust foundation for distributed consensus and threshold signature operations while maintaining clear separation of concerns and testability.

