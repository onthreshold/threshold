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
use oracle::oracle::Oracle;
use types::errors::NodeError;
use types::utxo::Utxo;

use super::Wallet;

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
}

impl TaprootWallet {
    #[must_use]
    pub fn new(oracle: Box<dyn Oracle>, addresses: Vec<Address>, network: Network) -> Self {
        Self {
            addresses,
            utxos: Vec::new(),
            oracle,
            network,
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
}
