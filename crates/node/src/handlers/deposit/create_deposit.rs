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
            deposit_tracking_id: deposit_tracking_id.clone(),
            deposit_address: deposit_address.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

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

        let broadcast_message = serde_json::json!({
            "deposit_address": deposit_address.to_string(),
            "amount_sat": amount_sat,
            "deposit_tracking_id": deposit_tracking_id,
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        if let Err(e) = node.network_handle.send_broadcast(
            IdentTopic::new("deposit-intents"),
            broadcast_message.to_string().as_bytes().to_vec(),
        ) {
            info!("Failed to broadcast new deposit address: {:?}", e);
        }

        Ok((deposit_tracking_id, deposit_address.to_string()))
    }

    pub fn get_pending_deposit_intents(&self) -> Vec<DepositIntent> {
        self.pending_intents.clone()
    }

    pub fn update_user_balance<N: Network, D: Db, O: Oracle>(
        &mut self,
        node: &mut NodeState<N, D, O>,
        tx: Transaction,
    ) -> Result<(), NodeError> {
        if !self.processed_txids.insert(tx.compute_txid()) {
            return Ok(());
        }

        // Extract the user's address from the script_sig which is a P2PKH scriptPubKey
        let user_address = Address::from_script(&tx.input[0].script_sig, BitcoinNetwork::Testnet)
            .map_err(|_| NodeError::Error("No input address found".to_string()))?;

        println!("tx.output: {:?}", tx.output);
        let deposit_amount = tx
            .output
            .iter()
            .filter_map(|output| {
                if let Ok(address) =
                    Address::from_script(&output.script_pubkey, BitcoinNetwork::Testnet)
                {
                    if self.deposit_addresses.contains(&address.to_string()) {
                        return Some(output.value.to_sat());
                    }
                }
                None
            })
            .sum::<u64>();

        println!("deposit_amount: {:?}", deposit_amount);
        let user_account = node
            .chain_state
            .get_account(&user_address.to_string())
            .ok_or(NodeError::Error("User not found".to_string()))?;

        println!("user_account: {:?}", user_account);
        let updated_account = user_account.update_balance(deposit_amount as i64);

        println!("updated_account: {:?}", updated_account.clone());
        node.chain_state
            .upsert_account(&user_address.to_string(), updated_account);

        Ok(())
    }
}
