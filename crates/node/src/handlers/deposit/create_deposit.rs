use std::{collections::HashSet, str::FromStr};

use bitcoin::{
    Address, Network as BitcoinNetwork, Transaction as BitcoinTransaction, hashes::Hash,
    secp256k1::Scalar,
};
use libp2p::gossipsub::IdentTopic;
use protocol::transaction::Transaction;
use tokio::sync::broadcast;
use tracing::{error, info};

use types::errors::NodeError;
use uuid::Uuid;

use crate::{
    NodeState, handlers::deposit::DepositIntentState, swarm_manager::Network, wallet::Wallet,
};
use types::intents::DepositIntent;

impl DepositIntentState {
    #[must_use]
    pub fn new(deposit_intent_tx: broadcast::Sender<DepositIntent>) -> Self {
        Self {
            deposit_addresses: HashSet::new(),
            deposit_intent_tx,
            processed_txids: HashSet::new(),
        }
    }

    pub fn create_deposit_from_intent<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        deposit_intent: DepositIntent,
    ) -> Result<(), NodeError> {
        node.chain_interface
            .insert_deposit_intent(deposit_intent.clone())?;

        node.wallet.add_address(
            Address::from_str(&deposit_intent.deposit_address)
                .map_err(|e| NodeError::Error(format!("Failed to parse deposit address: {e}")))?
                .assume_checked(),
        );

        if self
            .deposit_addresses
            .insert(deposit_intent.deposit_address.clone())
        {
            if let Err(e) = self.deposit_intent_tx.send(deposit_intent) {
                error!("Failed to notify deposit monitor of new address: {}", e);
            }
        }

        Ok(())
    }

    pub fn create_deposit<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        user_pubkey: &str,
        amount_sat: u64,
    ) -> Result<(String, String), NodeError> {
        let deposit_tracking_id = Uuid::new_v4().to_string();

        let Some(ref frost_pubkey) = node.pubkey_package else {
            return Err(NodeError::Error("No public key found".to_string()));
        };

        let frost_public_key = frost_pubkey
            .verifying_key()
            .serialize()
            .map_err(|x| NodeError::Error(format!("Failed to serialize public key: {x:?}")))?;

        let public_key = bitcoin::PublicKey::from_slice(&frost_public_key)
            .map_err(|e| NodeError::Error(format!("Failed to parse public key: {e}")))?;

        let tweak_scalar = Scalar::from_be_bytes(
            bitcoin::hashes::sha256::Hash::hash(deposit_tracking_id.as_bytes()).to_byte_array(),
        )
        .expect("32 bytes, should not fail");

        let deposit_address = node.wallet.generate_new_address(public_key, tweak_scalar);

        let deposit_intent = DepositIntent {
            amount_sat,
            user_pubkey: user_pubkey.to_string(),
            deposit_tracking_id: deposit_tracking_id.clone(),
            deposit_address: deposit_address.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                .as_secs(),
        };

        node.chain_interface
            .insert_deposit_intent(deposit_intent.clone())?;

        if self
            .deposit_addresses
            .insert(deposit_intent.deposit_address.clone())
        {
            if let Err(e) = self.deposit_intent_tx.send(deposit_intent.clone()) {
                error!("Failed to notify deposit monitor of new address: {}", e);
            }
        }

        if let Err(e) = node
            .network_handle
            .send_broadcast(IdentTopic::new("deposit-intents"), deposit_intent)
        {
            info!("Failed to broadcast new deposit address: {e:?}");
        }

        Ok((deposit_tracking_id, deposit_address.to_string()))
    }

    pub fn get_pending_deposit_intents<N: Network, W: Wallet>(
        &self,
        node: &NodeState<N, W>,
    ) -> Vec<DepositIntent> {
        match node.chain_interface.get_all_deposit_intents() {
            Ok(intents) => intents,
            Err(e) => {
                error!("Failed to fetch deposit intents from db: {}", e);
                Vec::new()
            }
        }
    }

    pub async fn update_user_balance<N: Network, W: Wallet>(
        &mut self,
        node: &mut NodeState<N, W>,
        tx: &BitcoinTransaction,
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

                if let Some(intent) = node
                    .chain_interface
                    .get_deposit_intent_by_address(&addr_str)
                {
                    info!(
                        "Updating user balance for address: {} amount: {}",
                        intent.user_pubkey,
                        output.value.to_sat()
                    );

                    let transaction = Transaction::create_deposit_transaction(
                        tx,
                        &intent.user_pubkey,
                        output.value.to_sat(),
                    )?;

                    node.chain_interface
                        .execute_transaction(transaction)
                        .await?;
                }
            }
        }

        node.wallet.ingest_external_tx(tx)?;

        Ok(())
    }
}
