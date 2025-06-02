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

/// Performs distributed key generation for FROST threshold signatures
///
/// This implements a 3-round DKG protocol where participants collaboratively
/// generate key shares without any trusted dealer.
///
/// # Arguments
/// * `max_signers` - Total number of participants
/// * `min_signers` - Minimum number of participants needed to sign (threshold)
///
/// # Returns
/// * `DkgResult` containing key packages for all participants and the public key package
///
/// # Security Notes
/// * In production, each participant should run this on their own secure environment
/// * Communication between participants must be authenticated and may need to be confidential
/// * Each participant should verify the integrity of received packages
pub fn perform_distributed_key_generation(
    max_signers: u16,
    min_signers: u16,
) -> Result<DkgResult, DkgError> {
    let rng = frost::rand_core::OsRng;

    if min_signers > max_signers {
        return Err(DkgError::InvalidParticipant(
            "min_signers cannot be greater than max_signers".to_string(),
        ));
    }

    if max_signers == 0 || min_signers == 0 {
        return Err(DkgError::InvalidParticipant(
            "Both max_signers and min_signers must be greater than 0".to_string(),
        ));
    }

    ////////////////////////////////////////////////////////////////////////////
    // Key generation, Round 1
    ////////////////////////////////////////////////////////////////////////////

    // Keep track of each participant's round 1 secret package.
    // In practice each participant will keep its copy; no one
    // will have all the participant's packages.
    let mut round1_secret_packages = BTreeMap::new();

    // Keep track of all round 1 packages sent to the given participant.
    // This is used to simulate the broadcast; in practice the packages
    // will be sent through some communication channel.
    let mut received_round1_packages = BTreeMap::new();

    // For each participant, perform the first part of the DKG protocol.
    // In practice, each participant will perform this on their own environments.
    for participant_index in 1..=max_signers {
        let participant_identifier =
            frost::Identifier::try_from(participant_index).map_err(|_| {
                DkgError::InvalidParticipant(format!(
                    "Invalid participant index: {}",
                    participant_index
                ))
            })?;

        let (round1_secret_package, round1_package) =
            frost::keys::dkg::part1(participant_identifier, max_signers, min_signers, rng)?;

        // Store the participant's secret package for later use.
        // In practice each participant will store it in their own environment.
        round1_secret_packages.insert(participant_identifier, round1_secret_package);

        // "Send" the round 1 package to all other participants. In this
        // test this is simulated using a BTreeMap; in practice this will be
        // sent through some communication channel.
        for receiver_participant_index in 1..=max_signers {
            if receiver_participant_index == participant_index {
                continue;
            }
            let receiver_participant_identifier =
                frost::Identifier::try_from(receiver_participant_index).map_err(|_| {
                    DkgError::InvalidParticipant(format!(
                        "Invalid receiver index: {}",
                        receiver_participant_index
                    ))
                })?;

            received_round1_packages
                .entry(receiver_participant_identifier)
                .or_insert_with(BTreeMap::new)
                .insert(participant_identifier, round1_package.clone());
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // Key generation, Round 2
    ////////////////////////////////////////////////////////////////////////////

    // Keep track of each participant's round 2 secret package.
    // In practice each participant will keep its copy; no one
    // will have all the participant's packages.
    let mut round2_secret_packages = BTreeMap::new();

    // Keep track of all round 2 packages sent to the given participant.
    // This is used to simulate the broadcast; in practice the packages
    // will be sent through some communication channel.
    let mut received_round2_packages = BTreeMap::new();

    // For each participant, perform the second part of the DKG protocol.
    // In practice, each participant will perform this on their own environments.
    for participant_index in 1..=max_signers {
        let participant_identifier =
            frost::Identifier::try_from(participant_index).map_err(|_| {
                DkgError::InvalidParticipant(format!(
                    "Invalid participant index: {}",
                    participant_index
                ))
            })?;

        let round1_secret_package = round1_secret_packages
            .remove(&participant_identifier)
            .ok_or_else(|| {
                DkgError::CommunicationError(format!(
                    "Missing round1 secret package for participant {}",
                    participant_index
                ))
            })?;

        let round1_packages = received_round1_packages
            .get(&participant_identifier)
            .ok_or_else(|| {
                DkgError::CommunicationError(format!(
                    "Missing round1 packages for participant {}",
                    participant_index
                ))
            })?;

        let (round2_secret_package, round2_packages) =
            frost::keys::dkg::part2(round1_secret_package, round1_packages)?;

        // Store the participant's secret package for later use.
        // In practice each participant will store it in their own environment.
        round2_secret_packages.insert(participant_identifier, round2_secret_package);

        // "Send" the round 2 package to all other participants. In this
        // test this is simulated using a BTreeMap; in practice this will be
        // sent through some communication channel.
        // Note that, in contrast to the previous part, here each other participant
        // gets its own specific package.
        for (receiver_identifier, round2_package) in round2_packages {
            received_round2_packages
                .entry(receiver_identifier)
                .or_insert_with(BTreeMap::new)
                .insert(participant_identifier, round2_package);
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // Key generation, final computation
    ////////////////////////////////////////////////////////////////////////////

    // Keep track of each participant's long-lived key package.
    // In practice each participant will keep its copy; no one
    // will have all the participant's packages.
    let mut key_packages = BTreeMap::new();

    // Keep track of each participant's public key package.
    // In practice, if there is a Coordinator, only they need to store the set.
    // If there is not, then all candidates must store their own sets.
    // All participants will have the same exact public key package.
    let mut pubkey_packages = BTreeMap::new();

    // For each participant, perform the third part of the DKG protocol.
    // In practice, each participant will perform this on their own environments.
    for participant_index in 1..=max_signers {
        let participant_identifier =
            frost::Identifier::try_from(participant_index).map_err(|_| {
                DkgError::InvalidParticipant(format!(
                    "Invalid participant index: {}",
                    participant_index
                ))
            })?;

        let round2_secret_package = round2_secret_packages
            .get(&participant_identifier)
            .ok_or_else(|| {
                DkgError::CommunicationError(format!(
                    "Missing round2 secret package for participant {}",
                    participant_index
                ))
            })?;

        let round1_packages = received_round1_packages
            .get(&participant_identifier)
            .ok_or_else(|| {
                DkgError::CommunicationError(format!(
                    "Missing round1 packages for participant {}",
                    participant_index
                ))
            })?;

        let round2_packages = received_round2_packages
            .get(&participant_identifier)
            .ok_or_else(|| {
                DkgError::CommunicationError(format!(
                    "Missing round2 packages for participant {}",
                    participant_index
                ))
            })?;

        let (key_package, pubkey_package) =
            frost::keys::dkg::part3(round2_secret_package, round1_packages, round2_packages)?;

        key_packages.insert(participant_identifier, key_package);
        pubkey_packages.insert(participant_identifier, pubkey_package);
    }

    // Verify all participants have the same public key package
    let first_pubkey_package = pubkey_packages.values().next().ok_or_else(|| {
        DkgError::CommunicationError("No public key packages generated".to_string())
    })?;

    for (participant_id, pubkey_package) in &pubkey_packages {
        if pubkey_package.verifying_key().serialize()?
            != first_pubkey_package.verifying_key().serialize()?
        {
            return Err(DkgError::CommunicationError(format!(
                "Participant {:?} has different public key package",
                participant_id
            )));
        }
    }

    Ok(DkgResult {
        key_packages,
        pubkey_package: first_pubkey_package.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dkg_3_of_5() {
        let result = perform_distributed_key_generation(5, 3);
        assert!(result.is_ok());

        let dkg_result = result.unwrap();
        assert_eq!(dkg_result.key_packages.len(), 5);

        // All participants should have valid key packages
        for (id, key_package) in &dkg_result.key_packages {
            assert_eq!(*key_package.identifier(), *id);
        }
    }

    #[test]
    fn test_dkg_2_of_3() {
        let result = perform_distributed_key_generation(3, 2);
        assert!(result.is_ok());

        let dkg_result = result.unwrap();
        assert_eq!(dkg_result.key_packages.len(), 3);
    }

    #[test]
    fn test_dkg_invalid_threshold() {
        // min_signers > max_signers should fail
        let result = perform_distributed_key_generation(3, 5);
        assert!(result.is_err());

        // Zero signers should fail
        let result = perform_distributed_key_generation(0, 0);
        assert!(result.is_err());
    }
}
