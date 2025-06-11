use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::witness::Witness;
use bitcoin::{Amount, ScriptBuf, Transaction, TxIn, TxOut};
use protocol::oracle::{Oracle, Utxo};
use types::errors::NodeError;

#[derive(Debug, Default)]
pub struct SimpleWallet<O: Oracle> {
    pub utxos: Vec<Utxo>,
    pub address: Option<bitcoin::Address>,
    pub oracle: O,
}

impl<O: Oracle> SimpleWallet<O> {
    pub async fn new(
        address: &bitcoin::Address,
        oracle: O,
        allow_unconfirmed: Option<bool>,
    ) -> Self {
        let client_utxos = oracle
            .refresh_utxos(address.clone(), 3, None, allow_unconfirmed.unwrap_or(false))
            .await
            .unwrap();

        let utxos = client_utxos
            .into_iter()
            .map(|u| Utxo {
                outpoint: u.outpoint,
                value: u.value,
                script_pubkey: u.script_pubkey,
            })
            .collect();

        Self {
            address: Some(address.clone()),
            utxos,
            oracle,
        }
    }

    pub fn create_spend(
        &mut self,
        amount_sat: u64,
        estimated_fee_sat: u64,
        address: &bitcoin::Address,
    ) -> Result<(Transaction, [u8; 32]), NodeError> {
        let total_needed = amount_sat + estimated_fee_sat;

        let pos = self
            .utxos
            .iter()
            .position(|u| u.value.to_sat() >= total_needed)
            .ok_or_else(|| {
                format!("No single UTXO large enough – need {} sats (amount: {} + fee: {}), coin selection not implemented", 
                        total_needed, amount_sat, estimated_fee_sat)
            }).map_err(|e| NodeError::Error(e.to_string()))?;

        let utxo = self.utxos.remove(pos);
        let change_sat = utxo.value.to_sat() - amount_sat - estimated_fee_sat;

        let input = TxIn {
            previous_output: utxo.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::ZERO,
            witness: Witness::new(),
        };

        let mut outputs = vec![TxOut {
            value: Amount::from_sat(amount_sat),
            script_pubkey: address.script_pubkey(),
        }];

        // Add change output if needed (only if change is meaningful, e.g., > dust threshold)
        if change_sat > 546 {
            // 546 sats is typical dust threshold for P2WPKH
            outputs.push(TxOut {
                value: Amount::from_sat(change_sat),
                script_pubkey: self.address.as_ref().unwrap().script_pubkey(),
            });
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

    pub fn sign(
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
}

#[derive(Debug)]
pub struct PendingSpend {
    pub tx: Transaction,
}
