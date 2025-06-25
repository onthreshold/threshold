use abci::db::Db;
use bitcoin::PublicKey;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Scalar;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::sighash::Prevouts;
use bitcoin::sighash::SighashCache;
use bitcoin::{Address, EcdsaSighashType};
use bitcoin::{
    Amount, Network, ScriptBuf, Sequence, Transaction, TxIn, TxOut, absolute::LockTime,
    transaction::Version, witness::Witness,
};
use itertools::Itertools;
use oracle::oracle::Oracle;
use protocol::block::Block;
use protocol::transaction::TransactionType;
use std::str::FromStr;
use std::sync::Arc;
use types::errors::NodeError;
use types::utxo::Utxo;

use super::Wallet;

const IN_SZ_VBYTES: f64 = 68.0; // assume P2WPKH/P2TR key-spend
const OUT_SZ_VBYTES: f64 = 31.0; // P2WPKH/P2TR output
const TX_OVH_VBYTES: f64 = 10.5; // version + locktime + marker/flag
const DUST: u64 = 546;

#[derive(Debug, Clone)]
pub struct TrackedUtxo {
    pub utxo: Utxo,
    pub address: bitcoin::Address,
}

pub struct TaprootWallet {
    pub addresses: Vec<bitcoin::Address>,
    pub utxos: Vec<TrackedUtxo>,
    pub oracle: Box<dyn Oracle>,
    pub network: Network,
    pub db: Option<Arc<dyn Db + Send + Sync>>,
}

impl TaprootWallet {
    #[must_use]
    pub fn new(oracle: Box<dyn Oracle>, addresses: Vec<Address>, network: Network) -> Self {
        Self {
            addresses,
            utxos: Vec::new(),
            oracle,
            network,
            db: None,
        }
    }

    #[must_use]
    pub fn new_with_db(
        oracle: Box<dyn Oracle>,
        addresses: Vec<Address>,
        network: Network,
        db: Arc<dyn Db + Send + Sync>,
    ) -> Self {
        let stored_utxos = db.get_utxos().unwrap_or_default();
        let mut tracked = Vec::new();
        for u in &stored_utxos {
            if let Ok(addr) = Address::from_script(&u.script_pubkey, network) {
                tracked.push(TrackedUtxo {
                    utxo: u.clone(),
                    address: addr,
                });
            }
        }

        Self {
            addresses,
            utxos: tracked,
            oracle,
            network,
            db: Some(db),
        }
    }

    fn select_utxos(&self, target: u64) -> Option<Vec<TrackedUtxo>> {
        let mut selected = Vec::new();
        let mut total_val: u64 = 0;
        let mut sorted_utxos = self.utxos.clone();
        sorted_utxos.sort_by(|a, b| b.utxo.value.cmp(&a.utxo.value));

        for utxo in sorted_utxos {
            if total_val < target {
                selected.push(utxo.clone());
                total_val += utxo.utxo.value.to_sat();
            } else {
                break;
            }
        }

        if total_val >= target {
            Some(selected)
        } else {
            None
        }
    }

    fn is_p2wpkh(script: &ScriptBuf) -> bool {
        let bytes = script.as_bytes();
        bytes.len() == 22 && bytes[0] == 0x00 && bytes[1] == 0x14
    }

    fn is_p2tr(script: &ScriptBuf) -> bool {
        let bytes = script.as_bytes();
        bytes.len() == 34 && bytes[0] == 0x51 && bytes[1] == 0x20
    }
}

#[async_trait::async_trait]
impl Wallet for TaprootWallet {
    fn add_address(&mut self, address: Address) {
        self.addresses.push(address);
    }

    async fn refresh_utxos(&mut self, allow_unconfirmed: Option<bool>) -> Result<(), NodeError> {
        self.utxos.clear();
        for addr in &self.addresses {
            let fetched = self
                .oracle
                .refresh_utxos(addr.clone(), 3, None, allow_unconfirmed.unwrap_or(false))
                .await?;

            if let Some(db) = &self.db {
                db.store_utxos(fetched.clone())?;
            }

            for u in fetched {
                self.utxos.push(TrackedUtxo {
                    utxo: u.clone(),
                    address: addr.clone(),
                });
            }
        }
        Ok(())
    }

    fn generate_new_address(&mut self, public_key: PublicKey, tweak: Scalar) -> bitcoin::Address {
        let secp = Secp256k1::new();
        let internal = public_key.inner.x_only_public_key().0;
        let (tweaked, _) = internal.add_tweak(&secp, &tweak).expect("tweak");
        let address = Address::p2tr(&secp, tweaked, None, self.network);
        self.addresses.push(address.clone());
        address
    }

    fn create_spend(
        &mut self,
        amount_sat: u64,
        estimated_fee_sat: u64,
        recipient: &bitcoin::Address,
        dry_run: bool,
    ) -> Result<(Transaction, [u8; 32]), NodeError> {
        let total_needed = amount_sat + estimated_fee_sat;
        let selected_utxos = self
            .select_utxos(total_needed)
            .ok_or_else(|| NodeError::Error("Not enough funds to create transaction".into()))?;

        let total_input_val = selected_utxos
            .iter()
            .fold(0, |acc, u| acc + u.utxo.value.to_sat());

        let change_address = self
            .utxos
            .iter()
            .min_by(|a, b| a.address.cmp(&b.address))
            .ok_or_else(|| NodeError::Error("No UTXOs selected".into()))?
            .address
            .clone();

        if !dry_run {
            let outpoints: Vec<_> = selected_utxos.iter().map(|u| u.utxo.outpoint).collect();
            self.utxos.retain(|t| !outpoints.contains(&t.utxo.outpoint));
        }

        let change_sat = total_input_val - amount_sat - estimated_fee_sat;

        let inputs: Vec<TxIn> = selected_utxos
            .iter()
            .map(|tracked_utxo| TxIn {
                previous_output: tracked_utxo.utxo.outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ZERO,
                witness: Witness::new(),
            })
            .collect();
        let mut outputs = vec![TxOut {
            value: Amount::from_sat(amount_sat),
            script_pubkey: recipient.script_pubkey(),
        }];

        if change_sat > 546 {
            outputs.push(TxOut {
                value: Amount::from_sat(change_sat),
                script_pubkey: change_address.script_pubkey(),
            });
        }

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: inputs,
            output: outputs,
        };

        if !dry_run && change_sat > 546 {
            let change_vout: u32 = tx
                .output
                .iter()
                .position(|o| o.script_pubkey == change_address.script_pubkey())
                .unwrap()
                .try_into()
                .unwrap();

            self.utxos.push(TrackedUtxo {
                utxo: Utxo {
                    outpoint: bitcoin::OutPoint {
                        txid: tx.compute_txid(),
                        vout: change_vout,
                    },
                    value: Amount::from_sat(change_sat),
                    script_pubkey: change_address.script_pubkey(),
                },
                address: change_address,
            });
        }

        let mut sighash_cache = SighashCache::new(&tx);

        let utxo_to_sign = selected_utxos
            .first()
            .ok_or_else(|| NodeError::Error("No UTXOs to sign".into()))?;

        let sighash = if Self::is_p2wpkh(&utxo_to_sign.utxo.script_pubkey) {
            sighash_cache
                .p2wpkh_signature_hash(
                    0,
                    &utxo_to_sign.utxo.script_pubkey,
                    utxo_to_sign.utxo.value,
                    EcdsaSighashType::All,
                )
                .map_err(|e| NodeError::Error(format!("Failed to calculate sighash: {e}")))?
                .to_byte_array()
        } else if Self::is_p2tr(&utxo_to_sign.utxo.script_pubkey) {
            let prevouts: Vec<TxOut> = selected_utxos
                .iter()
                .map(|u| bitcoin::TxOut {
                    value: u.utxo.value,
                    script_pubkey: u.utxo.script_pubkey.clone(),
                })
                .collect();
            sighash_cache
                .taproot_key_spend_signature_hash(
                    0,
                    &Prevouts::All(&prevouts),
                    bitcoin::TapSighashType::All,
                )
                .map_err(|e| NodeError::Error(format!("Failed to calculate sighash: {e}")))?
                .to_byte_array()
        } else {
            return Err(NodeError::Error("Unsupported script type".into()));
        };

        Ok((tx, sighash))
    }

    fn sign(
        &mut self,
        tx: &Transaction,
        private_key: &bitcoin::PrivateKey,
        sighash: [u8; 32],
    ) -> Transaction {
        // For P2WPKH, we need to create a witness signature
        let secp = bitcoin::key::Secp256k1::new();

        // Create the signature using the properly calculated sighash
        let message = bitcoin::secp256k1::Message::from_digest(sighash);
        let signature = secp.sign_ecdsa(&message, &private_key.inner);

        // Create witness with signature + sighash type (0x01 = SIGHASH_ALL)
        let mut sig_bytes = signature.serialize_der().to_vec();
        sig_bytes.push(0x01); // SIGHASH_ALL

        let compressed_pubkey = bitcoin::CompressedPublicKey::from_private_key(&secp, private_key)
            .expect("Failed to get compressed public key");

        let mut witness = Witness::new();
        witness.push(sig_bytes);
        witness.push(compressed_pubkey.to_bytes());

        let mut response_tx = tx.clone();
        // Add witness to the first (and only) input
        if let Some(input) = response_tx.input.first_mut() {
            input.witness = witness;
        }

        response_tx
    }

    fn ingest_external_tx(&mut self, tx: &Transaction) -> Result<(), NodeError> {
        self.utxos.retain(|t| {
            !tx.input
                .iter()
                .any(|i| i.previous_output == t.utxo.outpoint)
        });

        for (idx, out) in tx.output.iter().enumerate() {
            if let Some(addr) = self
                .addresses
                .iter()
                .find(|a| a.script_pubkey() == out.script_pubkey)
            {
                self.utxos.push(TrackedUtxo {
                    utxo: Utxo {
                        outpoint: bitcoin::OutPoint {
                            txid: tx.compute_txid(),
                            vout: u32::try_from(idx).unwrap(),
                        },
                        value: out.value,
                        script_pubkey: out.script_pubkey.clone(),
                    },
                    address: addr.clone(),
                });
            }
        }
        Ok(())
    }

    fn get_utxos(&self) -> Vec<TrackedUtxo> {
        self.utxos.clone()
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn get_transaction_for_block(
        &self,
        block: Block,
        feerate_sat_per_vb: u64,
    ) -> Result<bitcoin::Transaction, NodeError> {
        let mut payouts: Vec<(bitcoin::Address, u64)> = Vec::new();

        for tx in &block.body.transactions {
            if tx.r#type != TransactionType::Withdrawal {
                continue;
            }

            let meta = tx
                .metadata
                .as_ref()
                .ok_or_else(|| NodeError::Error("Withdrawal tx missing metadata".into()))?;

            let addr_str = meta
                .get("address_to")
                .and_then(|v| v.as_str())
                .ok_or_else(|| NodeError::Error("Withdrawal tx missing address_to".into()))?;

            let amt_sat = meta
                .get("amount_sat")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| NodeError::Error("Withdrawal tx missing amount_sat".into()))?;

            let addr = bitcoin::Address::from_str(addr_str)
                .map_err(|e| NodeError::Error(e.to_string()))
                .map(bitcoin::Address::assume_checked)?;
            payouts.push((addr, amt_sat));
        }

        if payouts.is_empty() {
            return Err(NodeError::Error(
                "Block contains no withdrawals to process".into(),
            ));
        }

        let fee_rate = feerate_sat_per_vb as f64;
        let total_payout: u64 = payouts.iter().map(|(_, v)| *v).sum();
        let utxos = &self.utxos; // shorthand

        if utxos.is_empty() {
            return Err(NodeError::Error("Wallet has no spendable UTXOs".into()));
        }

        let mut candidates = utxos.clone();
        candidates.sort_by(|a, b| b.utxo.value.cmp(&a.utxo.value));

        let mut chosen: Option<Vec<TrackedUtxo>> = None;
        let mut chosen_change = 0_u64;

        'outer: for k in 1..=candidates.len() {
            for idx_set in (0..candidates.len()).combinations(k) {
                let inputs: Vec<&TrackedUtxo> = idx_set.iter().map(|&i| &candidates[i]).collect();
                let in_sum: u64 = inputs.iter().map(|t| t.utxo.value.to_sat()).sum();

                let mut n_out = payouts.len() + 1;
                let mut weight = (n_out as f64).mul_add(
                    OUT_SZ_VBYTES,
                    (k as f64).mul_add(IN_SZ_VBYTES, TX_OVH_VBYTES),
                );
                let mut fee = (weight * fee_rate).ceil() as u64;

                if in_sum < total_payout + fee {
                    continue; // under-funded
                }

                let mut change = in_sum - total_payout - fee;
                if change > 0 && change < DUST {
                    n_out -= 1;
                    weight = (n_out as f64).mul_add(
                        OUT_SZ_VBYTES,
                        (k as f64).mul_add(IN_SZ_VBYTES, TX_OVH_VBYTES),
                    );
                    fee = (weight * fee_rate).ceil() as u64;
                    change = in_sum - total_payout - fee;
                }

                if change == 0 || change >= DUST {
                    chosen = Some(inputs.into_iter().cloned().collect());
                    chosen_change = change;
                    break 'outer;
                }
            }
        }

        let selected = chosen.ok_or_else(|| {
            NodeError::Error("Insufficient funds to satisfy withdrawals + fee".into())
        })?;

        let tx_inputs: Vec<TxIn> = selected
            .iter()
            .map(|t| TxIn {
                previous_output: t.utxo.outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ZERO,
                witness: Witness::new(),
            })
            .collect();

        let mut tx_outputs: Vec<TxOut> = payouts
            .iter()
            .map(|(addr, amt)| TxOut {
                value: Amount::from_sat(*amt),
                script_pubkey: addr.script_pubkey(),
            })
            .collect();

        if chosen_change > 0 {
            let change_addr = self
                .addresses
                .first()
                .ok_or_else(|| NodeError::Error("Wallet has no change address".into()))?;
            tx_outputs.push(TxOut {
                value: Amount::from_sat(chosen_change),
                script_pubkey: change_addr.script_pubkey(),
            });
        }

        let tx = bitcoin::Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: tx_inputs,
            output: tx_outputs,
        };

        Ok(tx)
    }
}
