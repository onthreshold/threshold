use std::collections::BTreeMap;
use libp2p::PeerId;
use frost_secp256k1::{
    self as frost,
    keys::dkg::{round1, round2},
    Identifier,
};

pub mod main_loop;
pub mod dkg;

pub struct NodeState<'a> {
    // DKG
    pub r1_secret_package: Option<round1::SecretPackage>,
    pub peer_id: PeerId,
    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,
    pub peers: Vec<PeerId>,
    pub swarm: &'a mut libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
    pub min_signers: u16,
    pub max_signers: u16,
    pub rng: frost::rand_core::OsRng,
    pub r2_secret_package: Option<round2::SecretPackage>,

    pub pubkey_package: Option<frost::keys::PublicKeyPackage>,
    pub private_key_package: Option<frost::keys::KeyPackage>,
}

impl<'a> NodeState<'a> {
    pub fn new(
        swarm: &'a mut libp2p::swarm::Swarm<crate::swarm_manager::MyBehaviour>,
        min_signers: u16,
        max_signers: u16,
    ) -> Self {
        // Node State
        let peer_id = *swarm.local_peer_id();

        NodeState {
            r1_secret_package: None,
            r2_secret_package: None,
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
        }
    }
}

pub fn peer_id_to_identifier(peer_id: &PeerId) -> Identifier {
    let bytes = peer_id.to_bytes();
    Identifier::derive(&bytes).unwrap()
}
