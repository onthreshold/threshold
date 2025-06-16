use std::str::FromStr;

use crate::{
    NodeState,
    db::Db,
    handlers::withdrawl::{SpendIntent, SpendIntentState},
    swarm_manager::{Network, SelfRequest},
    wallet::{PendingSpend, Wallet},
};
use bitcoin::{
    Transaction,
    key::Secp256k1,
    secp256k1::{Message, PublicKey, ecdsa::Signature},
};
use libp2p::gossipsub;
use protocol::oracle::Oracle;
use sha2::{Digest, Sha256};
use types::errors::NodeError;

impl SpendIntentState {
    pub async fn propose_withdrawal<N: Network, D: Db, O: Oracle, W: Wallet<O>>(
        &mut self,
        node: &mut NodeState<N, D, O, W>,
        withdrawal_intent: &SpendIntent,
    ) -> Result<(u64, String), NodeError> {
        let account = node.chain_state.get_account(&withdrawal_intent.public_key);
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
            true,
        )?;

        let vsize = tx.vsize();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let fee = (current_fee_per_vb * vsize as f64) as u64;
        let total_amount = withdrawal_intent.amount_sat + fee;

        let nonce: [u8; 16] = rand::random();
        let challenge = Sha256::digest(nonce).to_vec();
        let challenge_hex = hex::encode(challenge);

        self.pending_intents
            .insert(challenge_hex.clone(), (withdrawal_intent.clone(), fee));

        Ok((total_amount, challenge_hex))
    }

    fn verify_signature(
        message_hex: &str,
        signature_hex: &str,
        public_key_hex: &str,
    ) -> Result<bool, NodeError> {
        let public_key =
            PublicKey::from_str(public_key_hex).map_err(|e| NodeError::Error(e.to_string()))?;

        let signature_bytes =
            hex::decode(signature_hex).map_err(|e| NodeError::Error(e.to_string()))?;
        let signature =
            Signature::from_der(&signature_bytes).map_err(|e| NodeError::Error(e.to_string()))?;

        let msg_bytes = hex::decode(message_hex)
            .map_err(|e| NodeError::Error(format!("Invalid message hex: {e}")))?;
        if msg_bytes.len() != 32 {
            return Err(NodeError::Error("Message must be 32 bytes".to_string()));
        }

        let message =
            Message::from_digest_slice(&msg_bytes).map_err(|e| NodeError::Error(e.to_string()))?;

        let secp = Secp256k1::new();
        Ok(secp.verify_ecdsa(&message, &signature, &public_key).is_ok())
    }

    pub fn confirm_withdrawal<N: Network, D: Db, O: Oracle, W: Wallet<O>>(
        &mut self,
        node: &mut NodeState<N, D, O, W>,
        challenge: &str,
        signature: &str,
    ) -> Result<(), NodeError> {
        let Some((withdrawal_intent, fee)) = self.pending_intents.remove(challenge) else {
            return Err(NodeError::Error("Challenge not found".to_string()));
        };

        if !Self::verify_signature(challenge, signature, &withdrawal_intent.public_key)? {
            return Err(NodeError::Error("Invalid signature".to_string()));
        }

        node.network_handle
            .send_self_request(
                SelfRequest::Spend {
                    amount_sat: withdrawal_intent.amount_sat,
                    fee,
                    address_to: withdrawal_intent.address_to.clone(),
                    user_pubkey: withdrawal_intent.public_key,
                },
                false,
            )
            .map_err(|e| NodeError::Error(format!("Failed to send spend request: {e:?}")))?;

        Ok(())
    }

    pub async fn handle_signed_withdrawal<N: Network, D: Db, O: Oracle, W: Wallet<O>>(
        node: &mut NodeState<N, D, O, W>,
        tx: &Transaction,
        fee: u64,
        user_pubkey: String,
    ) -> Result<(), NodeError> {
        node.oracle.broadcast_transaction(tx).await?;
        let user_account = node
            .chain_state
            .get_account(&user_pubkey)
            .ok_or_else(|| NodeError::Error("User not found".to_string()))?;

        let updated_account =
            user_account.update_balance(-((tx.output[0].value.to_sat() + fee) as i64));

        node.chain_state
            .upsert_account(&user_pubkey, updated_account);

        let recipient_script = tx.output[0].script_pubkey.clone();

        let spend_intent = PendingSpend {
            tx: tx.clone(),
            user_pubkey: user_pubkey.clone(),
            recipient_script,
            fee,
        };

        node.network_handle
            .send_broadcast(
                gossipsub::IdentTopic::new("withdrawls"),
                bincode::encode_to_vec(&spend_intent, bincode::config::standard())
                    .map_err(|x| NodeError::Error(x.to_string()))?,
            )
            .map_err(|x| NodeError::Error(format!("Failed to send broadcast: {x:?}")))?;

        Ok(())
    }

    pub async fn handle_withdrawl_message<N: Network, D: Db, O: Oracle, W: Wallet<O>>(
        &self,
        node: &mut NodeState<N, D, O, W>,
        pending: PendingSpend,
    ) -> Result<(), NodeError> {
        node.oracle.broadcast_transaction(&pending.tx).await?;

        node.wallet.ingest_external_tx(&pending.tx)?;

        let pay_out = pending
            .tx
            .output
            .iter()
            .find(|o| o.script_pubkey == pending.recipient_script)
            .ok_or_else(|| NodeError::Error("payment output not found".into()))?;

        let debit = pay_out.value.to_sat() + pending.fee;

        let mut acct = node
            .chain_state
            .get_account(&pending.user_pubkey)
            .ok_or_else(|| NodeError::Error("user missing".into()))?
            .clone();
        acct = acct.update_balance(-(debit as i64));
        node.chain_state.upsert_account(&pending.user_pubkey, acct);

        Ok(())
    }
}
