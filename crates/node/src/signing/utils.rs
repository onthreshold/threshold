use std::collections::BTreeMap;

use frost_secp256k1::{self as frost};
use tracing::{error, info};
use types::errors::NodeError;

use crate::{NodeState, db::Db, signing::SigningState, swarm_manager::Network};

impl SigningState {
    pub fn new() -> Result<Self, NodeError> {
        Ok(SigningState {
            active_signing: None,
            pending_spends: BTreeMap::new(),
        })
    }

    pub fn frost_signature_to_bitcoin(
        frost_sig: &frost::Signature,
    ) -> Result<bitcoin::secp256k1::schnorr::Signature, String> {
        let sig_bytes = frost_sig
            .serialize()
            .map_err(|e| format!("Serialize frost sig: {}", e))?;

        let schnorr_bytes = match sig_bytes.len() {
            64 => sig_bytes,
            65 => sig_bytes[..64].to_vec(),
            _ => return Err(format!("Unsupported signature len {}", sig_bytes.len())),
        };

        bitcoin::secp256k1::schnorr::Signature::from_slice(&schnorr_bytes)
            .map_err(|e| format!("Parse schnorr sig: {}", e))
    }

    pub fn start_spend_request<N: Network, D: Db>(
        &mut self,
        node: &mut NodeState<N, D>,
        amount_sat: u64,
    ) -> Option<String> {
        info!("🚀 Creating spend request for {} sat", amount_sat);
        match node.wallet.create_spend(amount_sat) {
            Ok((tx, sighash)) => {
                let sighash_hex = hex::encode(sighash);
                match self.start_signing_session(node, &sighash_hex) {
                    Ok(_) => (),
                    Err(e) => {
                        error!("❌ Failed to start signing session: {}", e);
                        return None;
                    }
                }

                if let Some(active) = &self.active_signing {
                    self.pending_spends
                        .insert(active.sign_id, crate::wallet::PendingSpend { tx });
                    info!("🚀 Spend request prepared (session id {})", active.sign_id);

                    Some(hex::encode(sighash))
                } else {
                    error!("❌ Failed to start signing session");
                    None
                }
            }
            Err(e) => {
                error!("❌ Failed to create spend transaction: {}", e);
                None
            }
        }
    }
}
