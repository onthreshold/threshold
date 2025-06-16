use std::{collections::HashSet, str::FromStr};

use bitcoin::{Address, Network as BitcoinNetwork, Transaction, hashes::Hash, secp256k1::Scalar};
use libp2p::gossipsub::IdentTopic;
use protocol::chain_state::Account;
use tokio::sync::broadcast;
use tracing::{error, info};

use types::errors::NodeError;
use uuid::Uuid;

use crate::{
    NodeState, db::Db, handlers::deposit::DepositIntentState, swarm_manager::Network,
    wallet::Wallet,
};
use types::intents::DepositIntent;

impl DepositIntentState {
    pub fn new(deposit_intent_tx: broadcast::Sender<DepositIntent>) -> Self {
        Self {
            pending_intents: vec![],
            deposit_addresses: HashSet::new(),
            deposit_intent_tx,
            processed_txids: HashSet::new(),
        }
    }

    pub fn create_deposit_from_intent<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
        deposit_intent: DepositIntent,
    ) -> Result<(), NodeError> {
        node.db.insert_deposit_intent(deposit_intent.clone())?;

        node.wallet.add_address(
            Address::from_str(&deposit_intent.deposit_address)
                .map_err(|e| NodeError::Error(format!("Failed to parse deposit address: {}", e)))?
                .assume_checked(),
        );

        if self
            .deposit_addresses
            .insert(deposit_intent.deposit_address.clone())
        {
            if let Err(e) = self.deposit_intent_tx.send(deposit_intent.clone()) {
                error!("Failed to notify deposit monitor of new address: {}", e);
            }
        }

        Ok(())
    }

    pub async fn create_deposit<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
        user_pubkey: String,
        amount_sat: u64,
    ) -> Result<(String, String), NodeError> {
        let deposit_tracking_id = Uuid::new_v4().to_string();

        let Some(ref frost_pubkey) = node.pubkey_package else {
            return Err(NodeError::Error("No public key found".to_string()));
        };

        let frost_public_key = frost_pubkey
            .verifying_key()
            .serialize()
            .map_err(|x| NodeError::Error(format!("Failed to serialize public key: {:?}", x)))?;

        let public_key = bitcoin::PublicKey::from_slice(&frost_public_key)
            .map_err(|e| NodeError::Error(format!("Failed to parse public key: {}", e)))?;

        let tweak_scalar = Scalar::from_be_bytes(
            bitcoin::hashes::sha256::Hash::hash(deposit_tracking_id.as_bytes()).to_byte_array(),
        )
        .expect("32 bytes, should not fail");

        let deposit_address = node.wallet.generate_new_address(public_key, tweak_scalar);

        let deposit_intent = DepositIntent {
            amount_sat,
            user_pubkey: user_pubkey.clone(),
            deposit_tracking_id: deposit_tracking_id.clone(),
            deposit_address: deposit_address.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        if node.chain_state.get_account(&user_pubkey).is_none() {
            node.chain_state.upsert_account(
                &user_pubkey,
                protocol::chain_state::Account::new(user_pubkey.clone(), 0),
            );
        }

        node.db.insert_deposit_intent(deposit_intent.clone())?;

        if self
            .deposit_addresses
            .insert(deposit_intent.deposit_address.clone())
        {
            if let Err(e) = self.deposit_intent_tx.send(deposit_intent.clone()) {
                error!("Failed to notify deposit monitor of new address: {}", e);
            }
        }

        if let Err(e) = node.network_handle.send_broadcast(
            IdentTopic::new("deposit-intents"),
            bincode::encode_to_vec(&deposit_intent, bincode::config::standard())
                .map_err(|x| NodeError::Error(x.to_string()))?,
        ) {
            info!("Failed to broadcast new deposit address: {:?}", e);
        }

        Ok((deposit_tracking_id, deposit_address.to_string()))
    }

    pub fn get_pending_deposit_intents<N: Network, D: Db, W: Wallet>(
        &self,
        node: &NodeState<N, D, W>,
    ) -> Vec<DepositIntent> {
        match node.db.get_all_deposit_intents() {
            Ok(intents) => intents,
            Err(e) => {
                error!("Failed to fetch deposit intents from db: {}", e);
                Vec::new()
            }
        }
    }

    pub fn update_user_balance<N: Network, D: Db, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, D, W>,
        tx: Transaction,
    ) -> Result<(), NodeError> {
        if !self.processed_txids.insert(tx.compute_txid()) {
            return Ok(());
        }

        for output in &tx.output {
            if let Ok(address) =
                Address::from_script(&output.script_pubkey, BitcoinNetwork::Testnet)
            {
                let addr_str = address.to_string();
                if !self.deposit_addresses.contains(&addr_str) {
                    continue;
                }

                if let Some(intent) = node.db.get_deposit_intent_by_address(&addr_str)? {
                    info!(
                        "Updating user balance for address: {} amount: {}",
                        intent.user_pubkey,
                        output.value.to_sat()
                    );
                    let user_account = node
                        .chain_state
                        .get_account(&intent.user_pubkey.clone())
                        .cloned()
                        .unwrap_or(Account::new(intent.user_pubkey.clone(), 0));

                    let updated = user_account.update_balance(output.value.to_sat() as i64);
                    node.chain_state
                        .upsert_account(&intent.user_pubkey, updated);
                }
            }
        }

        node.wallet.ingest_external_tx(&tx)?;

        Ok(())
    }
}
