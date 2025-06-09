use frost_secp256k1 as frost;
use frost_secp256k1::keys::{KeyPackage, PublicKeyPackage};
use std::collections::BTreeMap;

/// Result of DKG process containing key packages for all participants
#[derive(Debug, Clone)]
pub struct DkgResult {
    /// Key packages for each participant (contains their secret shares)
    pub key_packages: BTreeMap<frost::Identifier, KeyPackage>,
    /// Public key package (same for all participants)
    pub pubkey_package: PublicKeyPackage,
}

/// Error types for DKG operations
#[derive(Debug)]
pub enum DkgError {
    FrostError(frost::Error),
    InvalidParticipant(String),
    CommunicationError(String),
}

impl std::fmt::Display for DkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DkgError::FrostError(e) => write!(f, "FROST error: {}", e),
            DkgError::InvalidParticipant(msg) => write!(f, "Invalid participant: {}", msg),
            DkgError::CommunicationError(msg) => write!(f, "Communication error: {}", msg),
        }
    }
}

impl std::error::Error for DkgError {}

impl From<frost::Error> for DkgError {
    fn from(e: frost::Error) -> Self {
        DkgError::FrostError(e)
    }
}

/// Perform a full dealer-less DKG entirely in-memory.
pub fn perform_distributed_key_generation(
    peers: Vec<frost::Identifier>,
    max_signers: u16,
    min_signers: u16,
) -> Result<DkgResult, DkgError> {
    use frost::keys::dkg::{round1, round2};

    if min_signers == 0 || max_signers == 0 || min_signers > max_signers {
        return Err(DkgError::InvalidParticipant(
            "Invalid signer parameters".to_string(),
        ));
    }
    let rng = frost::rand_core::OsRng;

    // Round1 secret/package maps
    let mut r1_secret: BTreeMap<_, round1::SecretPackage> = BTreeMap::new();
    let mut r1_pkg_sent: BTreeMap<_, BTreeMap<_, round1::Package>> = BTreeMap::new();

    for id in peers.clone() {
        let (sec, pkg) = frost::keys::dkg::part1(id, max_signers, min_signers, rng)?;
        r1_secret.insert(id, sec);
        // broadcast pkg to others
        for recv_id in peers.clone() {
            if recv_id == id {
                continue;
            }
            r1_pkg_sent
                .entry(recv_id)
                .or_default()
                .insert(id, pkg.clone());
        }
    }

    // Round2
    let mut r2_secret: BTreeMap<_, round2::SecretPackage> = BTreeMap::new();
    let mut r2_pkg_sent: BTreeMap<_, BTreeMap<_, round2::Package>> = BTreeMap::new();

    for id in peers.clone() {
        let sec1 = r1_secret.get(&id).unwrap().clone();
        let r1_recv = r1_pkg_sent.get(&id).unwrap();
        let (sec2, pkgs2) = frost::keys::dkg::part2(sec1, r1_recv)?;
        r2_secret.insert(id, sec2);
        for (recv_id, pkg) in pkgs2 {
            r2_pkg_sent.entry(recv_id).or_default().insert(id, pkg);
        }
    }

    // Round3, build key packages
    let mut key_pkgs = BTreeMap::new();
    let mut group_pub: Option<PublicKeyPackage> = None;

    for id in peers {
        let sec2 = r2_secret.get(&id).unwrap();
        let r1_recv = r1_pkg_sent.get(&id).unwrap();
        let r2_recv = r2_pkg_sent.get(&id).unwrap();
        let (key_pkg, pub_pkg) = frost::keys::dkg::part3(sec2, r1_recv, r2_recv)?;
        if group_pub.is_none() {
            group_pub = Some(pub_pkg.clone());
        }
        key_pkgs.insert(id, key_pkg);
    }

    Ok(DkgResult {
        key_packages: key_pkgs,
        pubkey_package: group_pub.expect("pubkey package"),
    })
}
