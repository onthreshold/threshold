use std::collections::{BTreeMap, HashSet};

use frost_secp256k1::{
    Identifier,
    keys::dkg::{round1, round2},
};
use libp2p::PeerId;

pub mod handler;
pub mod key_creation;
pub mod utils;

pub struct DkgState {
    pub dkg_started: bool,
    pub dkg_listeners: HashSet<PeerId>,
    pub round1_listeners: HashSet<PeerId>,

    pub start_dkg_topic: libp2p::gossipsub::IdentTopic,
    pub round1_topic: libp2p::gossipsub::IdentTopic,

    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,

    pub r1_secret_package: Option<round1::SecretPackage>,
    pub r2_secret_package: Option<round2::SecretPackage>,
}
