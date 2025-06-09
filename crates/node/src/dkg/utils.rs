use crate::dkg::DkgState;
use frost_secp256k1::{self as frost};
use libp2p::{PeerId, gossipsub};
use std::collections::{BTreeMap, HashSet};
use types::errors::NodeError;

impl DkgState {
    pub fn new(
        min_signers: u16,
        max_signers: u16,
        peer_id: PeerId,
        dkg_completed: bool,
    ) -> Result<Self, NodeError> {
        Ok(DkgState {
            min_signers,
            max_signers,
            rng: frost::rand_core::OsRng,
            peer_id,
            dkg_listeners: HashSet::new(),
            start_dkg_topic: gossipsub::IdentTopic::new("start-dkg"),
            round1_topic: gossipsub::IdentTopic::new("round1_topic"),
            round1_peer_packages: BTreeMap::new(),
            round2_peer_packages: BTreeMap::new(),
            r1_secret_package: None,
            r2_secret_package: None,
            dkg_started: false,
            dkg_completed,
        })
    }
}
