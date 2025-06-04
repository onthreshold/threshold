use frost_secp256k1::{
    self as frost, Identifier,
    keys::dkg::{round1, round2},
};
use libp2p::{PeerId, gossipsub, identity::Keypair};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use crate::swarm_manager::build_swarm;

pub mod dkg;
pub mod grpc_service;
pub mod main_loop;
pub mod signing;
pub mod swarm_manager;
pub mod wallet;

pub mod errors;

#[derive(Serialize, Deserialize, PartialEq)]
pub struct PeerData {
    pub name: String,
    pub public_key: String,
}

pub struct NodeState {
    pub allowed_peers: Vec<PeerId>,
    pub peers_to_names: BTreeMap<PeerId, String>,

    // DKG
    pub dkg_listeners: HashSet<PeerId>,
    pub start_dkg_topic: libp2p::gossipsub::IdentTopic,
    pub round1_topic: libp2p::gossipsub::IdentTopic,
    pub r1_secret_package: Option<round1::SecretPackage>,
    pub peer_id: PeerId,
    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,
    pub peers: Vec<PeerId>,
    pub swarm: libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
    pub keypair: Keypair,
    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub r2_secret_package: Option<round2::SecretPackage>,

    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,

    // FROST signing
    pub active_signing: Option<ActiveSigning>,
    pub wallet: crate::wallet::SimpleWallet,
    pub pending_spends: std::collections::BTreeMap<u64, crate::wallet::PendingSpend>,
}

impl NodeState {
    pub fn peer_name(&self, peer_id: &PeerId) -> String {
        self.peers_to_names
            .get(peer_id)
            .unwrap_or(&peer_id.to_string())
            .clone()
    }

    pub fn new(
        keypair: Keypair,
        peer_data: Vec<PeerData>,
        min_signers: u16,
        max_signers: u16,
    ) -> Self {
        // Node State
        let swarm = build_swarm(keypair.clone()).expect("Failed to build swarm");
        let peer_id = *swarm.local_peer_id();

        let allowed_peers: Vec<PeerId> = peer_data
            .iter()
            .map(|peer| peer.public_key.parse().unwrap())
            .collect();

        let peers_to_names: BTreeMap<PeerId, String> = peer_data
            .iter()
            .map(|peer| (peer.public_key.parse().unwrap(), peer.name.clone()))
            .collect();

        NodeState {
            allowed_peers,
            peers_to_names,
            dkg_listeners: HashSet::new(),
            round1_topic: gossipsub::IdentTopic::new("round1_topic"),
            start_dkg_topic: gossipsub::IdentTopic::new("start-dkg"),
            r1_secret_package: None,
            r2_secret_package: None,
            keypair,
            peer_id,
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            swarm,
            min_signers,
            max_signers,
            peers: Vec::new(),
            rng: frost::rand_core::OsRng,
            pubkey_package: None,
            private_key_package: None,
            active_signing: None,
            wallet: crate::wallet::SimpleWallet::new(),
            pending_spends: BTreeMap::new(),
        }
    }
}

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    Identifier::derive(&bytes).unwrap()
}

// Active signing session tracking
pub struct ActiveSigning {
    pub sign_id: u64,
    pub message: Vec<u8>,
    pub selected_peers: Vec<PeerId>,
    pub nonces: frost::round1::SigningNonces,
    pub commitments: BTreeMap<Identifier, frost::round1::SigningCommitments>,
    pub signature_shares: BTreeMap<Identifier, frost::round2::SignatureShare>,
    pub signing_package: Option<frost::SigningPackage>,
    pub is_coordinator: bool,
}
