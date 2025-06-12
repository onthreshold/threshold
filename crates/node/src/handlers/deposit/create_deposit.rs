use std::collections::HashSet;

use bitcoin::{Address, Network as BitcoinNetwork, Transaction, hashes::Hash, secp256k1::Scalar};
use libp2p::gossipsub::IdentTopic;
use tokio::sync::broadcast;
use tracing::{error, info};

use types::errors::NodeError;
use uuid::Uuid;

use crate::{
    NodeState,
    db::Db,
    handlers::deposit::{DepositIntent, DepositIntentState},
    swarm_manager::Network,
};
use protocol::oracle::Oracle;

impl DepositIntentState {
    pub fn new(
        deposit_intent_tx: broadcast::Sender<String>,
        transaction_rx: broadcast::Receiver<Transaction>,
    ) -> Self {
        Self {
            pending_intents: vec![],
            deposit_addresses: HashSet::new(),
            deposit_intent_tx,
            transaction_rx,
            processed_txids: HashSet::new(),
        }
    }

    pub fn create_deposit_from_intent<N: Network, D: Db, O: Oracle>(
        &mut self,
        node: &mut NodeState<N, D, O>,
        deposit_intent: DepositIntent,
    ) -> Result<(), NodeError> {
        node.db.insert_deposit_intent(deposit_intent.clone())?;

        if self
            .deposit_addresses
            .insert(deposit_intent.deposit_address.clone())
        {
            if let Err(e) = self
                .deposit_intent_tx
                .send(deposit_intent.deposit_address.clone())
            {
                error!("Failed to notify deposit monitor of new address: {}", e);
            }
        }

        Ok(())
    }

    pub async fn create_deposit<N: Network, D: Db, O: Oracle>(
        &mut self,
        node: &mut NodeState<N, D, O>,
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

        let secp = bitcoin::secp256k1::Secp256k1::new();

        let internal_key = public_key.inner.x_only_public_key().0;

        let tweak_scalar = Scalar::from_be_bytes(
            bitcoin::hashes::sha256::Hash::hash(deposit_tracking_id.as_bytes()).to_byte_array(),
        )
        .expect("32 bytes, should not fail");

        let (tweaked_key, _) = internal_key
            .add_tweak(&secp, &tweak_scalar)
            .map_err(|e| NodeError::Error(format!("Failed to add tweak: {:?}", e)))?;

        let is_testnet: bool = std::env::var("IS_TESTNET")
            .unwrap_or("false".to_string())
            .parse()
            .unwrap();

        let deposit_address = Address::p2tr(
            &secp,
            tweaked_key,
            None,
            if is_testnet {
                BitcoinNetwork::Testnet
            } else {
                BitcoinNetwork::Bitcoin
            },
        );

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
            if let Err(e) = self
                .deposit_intent_tx
                .send(deposit_intent.deposit_address.clone())
            {
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

    pub fn get_pending_deposit_intents<N: Network, D: Db, O: Oracle>(
        &self,
        node: &NodeState<N, D, O>,
    ) -> Vec<DepositIntent> {
        match node.db.get_all_deposit_intents() {
            Ok(intents) => intents,
            Err(e) => {
                error!("Failed to fetch deposit intents from db: {}", e);
                Vec::new()
            }
        }
    }

    pub fn update_user_balance<N: Network, D: Db, O: Oracle>(
        &mut self,
        node: &mut NodeState<N, D, O>,
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
                    let user_account = node
                        .chain_state
                        .get_account(&intent.user_pubkey)
                        .ok_or(NodeError::Error("User not found".into()))?;

                    let updated = user_account.update_balance(output.value.to_sat() as i64);
                    node.chain_state
                        .upsert_account(&intent.user_pubkey, updated);
                }
            }
        }

        Ok(())
    }
}
