use std::{collections::BTreeMap, str::FromStr};

use crate::{NodeState, handlers::signing::SigningState, wallet::Wallet};
use frost_secp256k1::{self as frost};
use tracing::{error, info};
use types::{errors::NodeError, intents::PendingSpend, network::network_protocol::Network};

impl SigningState {
    pub const fn new() -> Result<Self, NodeError> {
        Ok(Self {
            active_signing: None,
            pending_spends: BTreeMap::new(),
        })
    }

    pub fn frost_signature_to_bitcoin(
        frost_sig: &frost::Signature,
    ) -> Result<bitcoin::secp256k1::schnorr::Signature, String> {
        let sig_bytes = frost_sig
            .serialize()
            .map_err(|e| format!("Serialize frost sig: {e}"))?;

        let schnorr_bytes = match sig_bytes.len() {
            64 => sig_bytes,
            65 => sig_bytes[..64].to_vec(),
            _ => return Err(format!("Unsupported signature len {}", sig_bytes.len())),
        };

        bitcoin::secp256k1::schnorr::Signature::from_slice(&schnorr_bytes)
            .map_err(|e| format!("Parse schnorr sig: {e}"))
    }

    pub fn start_spend_request<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        amount_sat: u64,
        estimated_fee_sat: u64,
        address: &str,
        user_pubkey: String,
        dry_run: bool,
    ) -> Option<String> {
        info!("🚀 Creating spend request for {} sat", amount_sat);

        let addr = bitcoin::Address::from_str(address).ok()?.assume_checked();

        let (tx, sighash) =
            match node
                .wallet
                .create_spend(amount_sat, estimated_fee_sat, &addr, dry_run)
            {
                Ok(res) => res,
                Err(e) => {
                    error!("❌ Failed to create spend transaction: {:?}", e);
                    return None;
                }
            };

        let sighash_hex = hex::encode(sighash);
        if let Err(e) = self.start_signing_session(node, &sighash_hex) {
            error!("❌ Failed to start signing session: {}", e);
            return None;
        }

        if let Some(active) = &self.active_signing {
            let recipient_script = addr.script_pubkey();
            self.pending_spends.insert(
                active.sign_id,
                PendingSpend {
                    tx,
                    user_pubkey,
                    recipient_script,
                    fee: estimated_fee_sat,
                },
            );
            info!("🚀 Spend request prepared (session id {})", active.sign_id);
            Some(sighash_hex)
        } else {
            error!("❌ Signing session never became active");
            None
        }
    }
}
