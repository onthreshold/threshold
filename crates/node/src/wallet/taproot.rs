use bitcoin::PublicKey;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Scalar;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::sighash::SighashCache;
use bitcoin::{Address, EcdsaSighashType};
use bitcoin::{
    Amount, Network, ScriptBuf, Sequence, Transaction, TxIn, TxOut, absolute::LockTime,
    transaction::Version, witness::Witness,
};
use oracle::oracle::Oracle;
use types::errors::NodeError;
use types::utxo::Utxo;

use super::traits::Wallet;

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
    pub fn new(oracle: Box<dyn Oracle>, addresses: Vec<Address>, network: Network) -> Self {
        Self {
            addresses,
            utxos: Vec::new(),
            oracle,
            network,
        }
    }

    fn select_single_utxo(&self, target: u64) -> Option<&TrackedUtxo> {
        self.utxos.iter().find(|t| t.utxo.value.to_sat() >= target)
    }
}

#[async_trait::async_trait]
impl Wallet for TaprootWallet {
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
        let utxo_entry = self
            .select_single_utxo(total_needed)
            .ok_or_else(|| NodeError::Error("No UTXO large enough".into()))?;

        let utxo = utxo_entry.utxo.clone();
        let change_address = utxo_entry.address.clone();

        if !dry_run {
            self.utxos.retain(|t| t.utxo.outpoint != utxo.outpoint);
        }

        let change_sat = utxo.value.to_sat() - amount_sat - estimated_fee_sat;

        let input = TxIn {
            previous_output: utxo.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ZERO,
            witness: Witness::new(),
        };

        let mut outputs = vec![TxOut {
            value: Amount::from_sat(amount_sat),
            script_pubkey: recipient.script_pubkey(),
        }];

        if change_sat > 546 {
            outputs.push(TxOut {
                value: Amount::from_sat(change_sat),
                script_pubkey: change_address.script_pubkey(),
            });
            if !dry_run {
                self.utxos.push(TrackedUtxo {
                    utxo: Utxo {
                        outpoint: utxo.outpoint,
                        value: Amount::from_sat(change_sat),
                        script_pubkey: change_address.script_pubkey(),
                    },
                    address: change_address.clone(),
                });
            }
        }

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![input],
            output: outputs,
        };

        let mut sighash_cache = SighashCache::new(&tx);
        let sighash = sighash_cache
            .p2wpkh_signature_hash(
                0, // input index
                &utxo.script_pubkey,
                utxo.value,
                EcdsaSighashType::All,
            )
            .map_err(|e| NodeError::Error(format!("Failed to calculate sighash: {}", e)))?;

        Ok((tx, sighash.to_byte_array()))
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
                            vout: idx as u32,
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
}
