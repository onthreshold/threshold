pub mod create_signature;
pub mod handler;
pub mod utils;
use std::collections::BTreeMap;

use frost_secp256k1::{self as frost, Identifier};
use libp2p::PeerId;
use types::intents::PendingSpend;

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

pub struct SigningState {
    pub active_signing: Option<ActiveSigning>,
    pub pending_spends: std::collections::BTreeMap<u64, PendingSpend>,
}
