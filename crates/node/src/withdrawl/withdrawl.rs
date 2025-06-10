use std::str::FromStr;

use crate::{
    NodeState,
    db::Db,
    swarm_manager::Network,
    withdrawl::{SpendIntent, SpendIntentState},
};
use bitcoin::{
    key::Secp256k1,
    secp256k1::{Message, PublicKey, ecdsa::Signature},
};
use protocol::oracle::Oracle;
use sha2::{Digest, Sha256, digest};
use types::errors::NodeError;

impl SpendIntentState {
    pub async fn propose_withdrawal<N: Network, D: Db, O: Oracle>(
        &mut self,
        node: &mut NodeState<N, D, O>,
        withdrawal_intent: &SpendIntent,
    ) -> Result<(u64, String), NodeError> {
        let account = node.chain_state.get_account(&withdrawal_intent.address_to);
        let Some(account) = account else {
            return Err(NodeError::Error("Account not found".to_string()));
        };

        if account.balance < withdrawal_intent.amount_sat {
            return Err(NodeError::Error("Insufficient balance".to_string()));
        }

        let current_fee_per_vb = node
            .oracle
            .get_current_fee_per_vb(withdrawal_intent.blocks_to_confirm.map(|b| b as u16))
            .await?;

        let (tx, _) = node.wallet.create_spend(
            withdrawal_intent.amount_sat,
            (current_fee_per_vb * 120.0) as u64, // Just estimate for now this doesnt affect vsize
            &bitcoin::Address::from_str(&withdrawal_intent.address_to)
                .unwrap()
                .assume_checked(),
        )?;

        let vsize = tx.vsize();
        let fee = (current_fee_per_vb * vsize as f64) as u64;
        let total_amount = withdrawal_intent.amount_sat + fee;

        let nonce: [u8; 16] = rand::random();
        let challenge = Sha256::digest(&nonce).to_vec();
        let challenge_hex = hex::encode(challenge);

        self.pending_intents
            .insert(challenge_hex.clone(), withdrawal_intent.clone());

        Ok((total_amount, challenge_hex))
    }

    fn verify_signature(
        message: &str,
        signature_hex: &str,
        public_key_hex: &str,
    ) -> Result<bool, NodeError> {
        let secp = Secp256k1::new();

        // Parse public key
        let public_key =
            PublicKey::from_str(public_key_hex).map_err(|e| NodeError::Error(e.to_string()))?;

        // Parse signature
        let signature_bytes =
            hex::decode(signature_hex).map_err(|e| NodeError::Error(e.to_string()))?;
        let signature =
            Signature::from_der(&signature_bytes).map_err(|e| NodeError::Error(e.to_string()))?;

        // Hash the message (Bitcoin uses double SHA256)
        let message_hash = Sha256::digest(message);
        let message_bytes =
            hex::decode(message_hash).map_err(|e| NodeError::Error(e.to_string()))?;
        let message = Message::from_digest_slice(&message_bytes)
            .map_err(|e| NodeError::Error(e.to_string()))?;

        // Verify signature
        Ok(secp.verify_ecdsa(&message, &signature, &public_key).is_ok())
    }

    pub async fn confirm_withdrawal<N: Network, D: Db, O: Oracle>(
        &mut self,
        node: &mut NodeState<N, D, O>,
        challenge: &str,
        signature: &str,
    ) -> Result<(), NodeError> {
        let Some(withdrawal_intent) = self.pending_intents.remove(challenge) else {
            return Err(NodeError::Error("Challenge not found".to_string()));
        };

        let is_valid = Self::verify_signature(challenge, signature, &withdrawal_intent.public_key)?;
        if !is_valid {
            return Err(NodeError::Error("Invalid signature".to_string()));
        }

        let tx = node.wallet.create_spend(
            withdrawal_intent.amount_sat,
            200,
            &bitcoin::Address::from_str(&withdrawal_intent.address_to)
                .unwrap()
                .assume_checked(),
        )?;

        node.wallet.broadcast_transaction(&tx)?;

        Ok(())
    }
}
