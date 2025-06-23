use crate::{
    handlers::{
        Handler, balance::BalanceState, consensus::ConsensusState, deposit::DepositIntentState,
        dkg::DkgState, signing::SigningState, withdrawl::SpendIntentState,
    },
    wallet::Wallet,
};
use abci::{ChainMessage, ChainResponse};
use frost_secp256k1::{self as frost, Identifier};
use libp2p::PeerId;
use oracle::oracle::Oracle;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::broadcast;
use tracing::{error, info};
use types::network::network_protocol::Network;
use types::{errors::NodeError, intents::DepositIntent, network::network_event::NetworkEvent};

pub use config::{ConfigStore, KeyStore, NodeConfig, NodeConfigBuilder};

pub mod config;
pub mod handlers;
pub mod main_loop;
pub mod start_node;

pub mod utils;
pub use utils::key_manager;
pub use utils::swarm_manager;
pub mod wallet;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerData {
    pub name: String,
    pub public_key: String,
}

pub struct NodeState<N: Network, W: Wallet> {
    pub handlers: Vec<Box<dyn Handler<N, W>>>,
    pub peer_id: PeerId,
    pub peers: HashSet<PeerId>,

    pub rng: frost::rand_core::OsRng,
    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,

    // FROST signing
    pub wallet: W,
    pub config: NodeConfig,
    pub network_handle: N,
    pub network_events_stream: broadcast::Receiver<NetworkEvent>,

    pub oracle: Box<dyn Oracle>,
    pub chain_interface_tx: messenger::Sender<ChainMessage, ChainResponse>,
}

impl<N: Network, W: Wallet> NodeState<N, W> {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::needless_pass_by_value)]
    pub async fn new_from_config(
        network_handle: &N,
        config: NodeConfig,
        network_events_sender: &broadcast::Sender<NetworkEvent>,
        deposit_intent_tx: broadcast::Sender<DepositIntent>,
        oracle: Box<dyn Oracle>,
        wallet: W,
        mut chain_interface_tx: messenger::Sender<ChainMessage, ChainResponse>,
    ) -> Result<Self, NodeError> {
        let keys = config.load_dkg_keys()?;
        let dkg_state = DkgState::new();
        let signing_state = SigningState::new();
        let mut consensus_state = ConsensusState::new();

        for peer in &config.allowed_peers {
            if let Ok(peer_id) = peer.public_key.parse::<PeerId>() {
                consensus_state.validators.insert(peer_id);
            }
        }

        consensus_state.validators.insert(network_handle.peer_id());

        let mut deposit_intent_state = DepositIntentState::new(deposit_intent_tx);
        let withdrawl_intent_state = SpendIntentState::new();
        let balance_state = BalanceState::new();

        if let Ok(ChainResponse::GetAllDepositIntents { intents }) = chain_interface_tx
            .send_message_with_response(ChainMessage::GetAllDepositIntents)
            .await
        {
            info!("Found {} deposit intents", intents.len());
            for intent in intents {
                if deposit_intent_state
                    .deposit_addresses
                    .insert(intent.deposit_address.clone())
                {
                    if let Err(e) = deposit_intent_state.deposit_intent_tx.send(intent.clone()) {
                        error!("Failed to notify deposit monitor of new address: {}", e);
                    }
                }
            }
        }

        let mut node_state = Self {
            network_handle: network_handle.clone(),
            network_events_stream: network_events_sender.subscribe(),
            peer_id: network_handle.peer_id(),
            peers: HashSet::new(),
            rng: frost::rand_core::OsRng,
            wallet,
            config,
            handlers: vec![
                Box::new(dkg_state),
                Box::new(signing_state),
                Box::new(consensus_state),
                Box::new(deposit_intent_state),
                Box::new(withdrawl_intent_state),
                Box::new(balance_state),
            ],
            pubkey_package: None,
            private_key_package: None,
            oracle,
            chain_interface_tx,
        };

        if let Some((private_key, pubkey)) = keys {
            node_state.private_key_package = Some(private_key);
            node_state.pubkey_package = Some(pubkey);
        }

        Ok(node_state)
    }
}

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    match Identifier::derive(&bytes) {
        Ok(identifier) => identifier,
        Err(e) => {
            error!("Failed to derive identifier: {}", e);
            panic!("Failed to derive identifier");
        }
    }
}
