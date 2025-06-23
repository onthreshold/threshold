use std::collections::{BTreeMap, HashSet};

use frost_secp256k1::{
    Identifier,
    keys::dkg::{round1, round2},
};
use libp2p::PeerId;

pub mod handler;
pub mod key_creation;

pub struct DkgState {
    pub dkg_started: bool,
    pub dkg_listeners: HashSet<PeerId>,
    pub round1_listeners: HashSet<PeerId>,

    pub round1_peer_packages: BTreeMap<Identifier, round1::Package>,
    pub round2_peer_packages: BTreeMap<Identifier, round2::Package>,

    pub r1_secret_package: Option<round1::SecretPackage>,
    pub r2_secret_package: Option<round2::SecretPackage>,
}

impl Default for DkgState {
    fn default() -> Self {
        Self::new()
    }
}

impl DkgState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dkg_listeners: HashSet::new(),
            round1_listeners: HashSet::new(),
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            r1_secret_package: None,
            r2_secret_package: None,
            dkg_started: false,
        }
    }
}
