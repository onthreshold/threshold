use bitcoin::{
    Address, Amount, Network, ScriptBuf,
    absolute::LockTime,
    hashes::Hash,
    secp256k1::{Secp256k1, XOnlyPublicKey},
    sighash::{Prevouts, SighashCache},
    taproot::TaprootSpendInfo,
    transaction::{Transaction, TxIn, TxOut},
    witness::Witness,
};
use frost_secp256k1 as frost;
use frost_secp256k1::keys::{KeyPackage, PublicKeyPackage};
use std::collections::{BTreeMap, HashMap};

use crate::dkg::{DkgResult, perform_distributed_key_generation};

/// Represents a UTXO that can be spent with FROST threshold signatures
#[derive(Debug, Clone)]
pub struct Utxo {
    pub outpoint: bitcoin::transaction::OutPoint,
    pub output: TxOut,
    pub block_height: Option<u32>,
}

/// Simplified FROST + Taproot wallet using ZCash Foundation's FROST library
#[derive(Debug)]
pub struct FrostTaprootWallet {
    /// FROST key packages for each participant (contains signing shares)
    key_packages: HashMap<frost::Identifier, KeyPackage>,

    /// FROST public key package (contains group public key and verifying shares)
    pub pubkey_package: PublicKeyPackage,

    /// Secp256k1 context for Bitcoin operations
    secp: Secp256k1<bitcoin::secp256k1::All>,

    /// The Taproot spending info
    taproot_spend_info: TaprootSpendInfo,

    /// The Taproot address
    taproot_address: Address,

    /// Threshold configuration
    pub min_signers: u16,
    pub max_signers: u16,
}

impl FrostTaprootWallet {
    /// Create a new FROST Taproot wallet with specified threshold using DKG
    pub fn new(min_signers: u16, max_signers: u16) -> Result<Self, Box<dyn std::error::Error>> {
        // Perform distributed key generation
        let dkg_result = perform_distributed_key_generation(max_signers, min_signers)?;

        Self::from_dkg_result(dkg_result, min_signers, max_signers)
    }

    /// Create wallet from existing DKG result (useful for testing or when DKG is done separately)
    pub fn from_dkg_result(
        dkg_result: DkgResult,
        min_signers: u16,
        max_signers: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let secp = Secp256k1::new();

        // Convert BTreeMap to HashMap for easier access
        let key_packages: HashMap<frost::Identifier, KeyPackage> =
            dkg_result.key_packages.into_iter().collect();

        // Convert FROST group public key to Bitcoin XOnlyPublicKey
        let group_public_key =
            Self::frost_pubkey_to_xonly(dkg_result.pubkey_package.verifying_key())?;

        // For FROST, we use key-path spending only (no script tree)
        // This is the most common and efficient approach for threshold signatures
        let taproot_spend_info = TaprootSpendInfo::new_key_spend(
            &secp,
            group_public_key,
            None, // No merkle root for key-path only
        );

        let taproot_address = Address::p2tr(
            &secp,
            group_public_key,
            None, // No merkle root for key-path spending
            Network::Bitcoin,
        );

        Ok(Self {
            key_packages,
            pubkey_package: dkg_result.pubkey_package,
            secp,
            taproot_spend_info,
            taproot_address,
            min_signers,
            max_signers,
        })
    }

    /// Convert FROST public key to Bitcoin XOnlyPublicKey
    fn frost_pubkey_to_xonly(
        frost_pubkey: &frost::VerifyingKey,
    ) -> Result<XOnlyPublicKey, Box<dyn std::error::Error>> {
        // Get the raw bytes from FROST public key
        let pubkey_bytes = frost_pubkey.serialize()?;

        // FROST public keys should be 33 bytes (compressed SEC1 format)
        if pubkey_bytes.len() != 33 {
            return Err(format!(
                "Invalid FROST public key length: {} bytes",
                pubkey_bytes.len()
            )
            .into());
        }
        let pubkey = bitcoin::secp256k1::PublicKey::from_slice(&pubkey_bytes)?;
        let (x_only_pubkey, _parity) = pubkey.x_only_public_key();

        Ok(x_only_pubkey)
    }

    /// Get the wallet's Taproot address
    pub fn address(&self) -> &Address {
        &self.taproot_address
    }

    /// Get list of participant identifiers
    pub fn participants(&self) -> Vec<frost::Identifier> {
        self.key_packages.keys().cloned().collect()
    }

    /// Create a spending transaction for a UTXO
    pub fn create_spending_transaction(
        &self,
        utxo: &Utxo,
        recipient_address: &Address,
        amount: Amount,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let input = TxIn {
            previous_output: utxo.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::ZERO,
            witness: Witness::new(),
        };

        let output = TxOut {
            value: amount,
            script_pubkey: recipient_address.script_pubkey(),
        };

        Ok(Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![input],
            output: vec![output],
        })
    }

    /// Create sighash for the transaction
    pub fn create_sighash(
        &self,
        tx: &Transaction,
        utxo: &Utxo,
    ) -> Result<[u8; 32], Box<dyn std::error::Error>> {
        let prevouts = vec![&utxo.output];
        let prevouts = Prevouts::All(&prevouts);

        let mut sighash_cache = SighashCache::new(tx);
        let sighash = sighash_cache.taproot_key_spend_signature_hash(
            0,
            &prevouts,
            bitcoin::sighash::TapSighashType::Default,
        )?;

        Ok(sighash.to_byte_array())
    }

    /// Convert FROST signature to Bitcoin Schnorr signature
    fn frost_signature_to_bitcoin(
        frost_sig: &frost::Signature,
    ) -> Result<bitcoin::secp256k1::schnorr::Signature, Box<dyn std::error::Error>> {
        let sig_bytes = frost_sig.serialize()?;

        // Handle different FROST signature formats
        let schnorr_bytes = match sig_bytes.len() {
            64 => {
                // Already in correct Schnorr format (r || s)
                sig_bytes
            }
            65 => {
                // FROST might include recovery ID or different encoding
                // Try removing the last byte (common for recovery ID)
                sig_bytes[..64].to_vec()
            }
            _ => {
                return Err(format!(
                    "Unsupported FROST signature length: {} bytes",
                    sig_bytes.len()
                )
                .into());
            }
        };

        bitcoin::secp256k1::schnorr::Signature::from_slice(&schnorr_bytes).map_err(|e| {
            format!(
                "Failed to convert FROST signature to Bitcoin Schnorr: {}",
                e
            )
            .into()
        })
    }

    /// FROST Round 1: Generate nonces and commitments for signing participants
    pub fn frost_round_1(
        &self,
        signing_participants: &[frost::Identifier],
    ) -> Result<
        (
            BTreeMap<frost::Identifier, frost::round1::SigningNonces>,
            BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
        ),
        Box<dyn std::error::Error>,
    > {
        if signing_participants.len() < self.min_signers as usize {
            return Err("Insufficient participants for threshold".into());
        }

        let mut rng = frost::rand_core::OsRng;
        let mut nonces_map = BTreeMap::new();
        let mut commitments_map = BTreeMap::new();

        // Each participant generates nonces and commitments
        for &participant_id in signing_participants {
            if let Some(key_package) = self.key_packages.get(&participant_id) {
                let (nonces, commitments) =
                    frost::round1::commit(key_package.signing_share(), &mut rng);
                nonces_map.insert(participant_id, nonces);
                commitments_map.insert(participant_id, commitments);
            } else {
                return Err(format!(
                    "Key package not found for participant: {:?}",
                    participant_id
                )
                .into());
            }
        }

        Ok((nonces_map, commitments_map))
    }

    /// FROST Round 2: Create signing package and generate signature shares
    pub fn frost_round_2(
        &self,
        message: &[u8],
        signing_participants: &[frost::Identifier],
        nonces_map: &BTreeMap<frost::Identifier, frost::round1::SigningNonces>,
        commitments_map: &BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
    ) -> Result<
        (
            frost::SigningPackage,
            BTreeMap<frost::Identifier, frost::round2::SignatureShare>,
        ),
        Box<dyn std::error::Error>,
    > {
        // Create signing package (coordinator's job)
        let signing_package = frost::SigningPackage::new(commitments_map.clone(), message);

        let mut signature_shares = BTreeMap::new();

        // Each participant creates their signature share
        for &participant_id in signing_participants {
            if let (Some(key_package), Some(nonces)) = (
                self.key_packages.get(&participant_id),
                nonces_map.get(&participant_id),
            ) {
                let signature_share = frost::round2::sign(&signing_package, nonces, key_package)?;
                signature_shares.insert(participant_id, signature_share);
            }
        }

        Ok((signing_package, signature_shares))
    }

    /// Complete end-to-end signing process using proper FROST workflow
    pub fn sign_transaction(
        &self,
        utxo: &Utxo,
        recipient_address: &Address,
        amount: Amount,
        signing_participants: Vec<frost::Identifier>,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        // 1. Create the spending transaction
        let mut tx = self
            .create_spending_transaction(utxo, recipient_address, amount)
            .map_err(|e| format!("Failed to create spending transaction: {}", e))?;

        // 2. Create the message to sign (sighash)
        let sighash = self
            .create_sighash(&tx, utxo)
            .map_err(|e| format!("Failed to create sighash: {}", e))?;

        // 3. FROST Round 1: Generate nonces and commitments
        let (nonces_map, commitments_map) = self
            .frost_round_1(&signing_participants)
            .map_err(|e| format!("FROST Round 1 failed: {}", e))?;

        // 4. FROST Round 2: Create signing package and generate signature shares
        let (signing_package, signature_shares) = self
            .frost_round_2(
                &sighash,
                &signing_participants,
                &nonces_map,
                &commitments_map,
            )
            .map_err(|e| format!("FROST Round 2 failed: {}", e))?;

        // 5. Aggregate signature shares into final signature
        let group_signature =
            frost::aggregate(&signing_package, &signature_shares, &self.pubkey_package)
                .map_err(|e| format!("FROST aggregation failed: {}", e))?;

        // 6. Convert FROST signature to Bitcoin format
        let bitcoin_signature = Self::frost_signature_to_bitcoin(&group_signature)
            .map_err(|e| format!("Signature conversion failed: {}", e))?;

        // 7. Add signature to witness (key-path spending)
        let mut witness = Witness::new();
        witness.push(bitcoin_signature.as_ref());
        tx.input[0].witness = witness;

        // 8. Verify the signature (optional verification step)
        let is_valid = self
            .pubkey_package
            .verifying_key()
            .verify(&sighash, &group_signature)
            .is_ok();

        if !is_valid {
            return Err("Generated signature is invalid".into());
        }

        Ok(tx)
    }

    /// Print wallet information
    pub fn print_info(&self) {
        println!("=== FROST Taproot Wallet ===");
        println!("Threshold: {}-of-{}", self.min_signers, self.max_signers);
        println!("Taproot Address: {}", self.taproot_address);

        let group_pubkey = self.pubkey_package.verifying_key();
        println!("Group Public Key: {:?}", group_pubkey);

        println!("Participants:");
        for (i, participant_id) in self.participants().iter().enumerate() {
            println!("  {}: {:?}", i + 1, participant_id);
        }
    }
}
