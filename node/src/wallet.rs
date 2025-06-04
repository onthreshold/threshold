use bitcoin::absolute::LockTime;
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::hex::DisplayHex;
use bitcoin::transaction::{OutPoint, Version};
use bitcoin::witness::Witness;
use bitcoin::{Amount, ScriptBuf, Transaction, TxIn, TxOut, hashes::sha256};
use hex;
use bs58;

use crate::NodeState;
use frost_secp256k1::{self as frost};

/// Very simple demonstration UTXO representation (key-path Taproot assumed)
#[derive(Debug, Clone)]
pub struct Utxo {
    pub outpoint: OutPoint,
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}

/// Wallet that only tracks a list of local UTXOs and is able to construct a
/// single-input spending transaction that possibly creates a change output. No
/// fee calculation is performed – this is purely for demonstration purposes.
#[derive(Debug)]
pub struct SimpleWallet {
    pub utxos: Vec<Utxo>,
}

impl Default for SimpleWallet {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleWallet {
    pub fn new() -> Self {
        Self {
            utxos: vec![
                Utxo {
                    outpoint: OutPoint {
                        txid: bitcoin::Txid::from_slice(&[0u8; 32]).unwrap(),
                        vout: 0,
                    },
                    value: Amount::from_sat(100_000),
                    script_pubkey: ScriptBuf::new(),
                },
                Utxo {
                    outpoint: OutPoint {
                        txid: bitcoin::Txid::from_slice(&[1u8; 32]).unwrap(),
                        vout: 0,
                    },
                    value: Amount::from_sat(50_000),
                    script_pubkey: ScriptBuf::new(),
                },
                Utxo {
                    outpoint: OutPoint {
                        txid: bitcoin::Txid::from_slice(&[2u8; 32]).unwrap(),
                        vout: 0,
                    },
                    value: Amount::from_sat(20_000),
                    script_pubkey: ScriptBuf::new(),
                },
            ],
        }
    }

    pub fn create_spend(&mut self, amount_sat: u64) -> Result<(Transaction, [u8; 32]), String> {
        let pos = self
            .utxos
            .iter()
            .position(|u| u.value.to_sat() >= amount_sat)
            .ok_or_else(|| {
                "No single UTXO large enough – coin selection not implemented".to_string()
            })?;

        let utxo = self.utxos.remove(pos);
        let change_sat = utxo.value.to_sat() - amount_sat;

        let input = TxIn {
            previous_output: utxo.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::ZERO,
            witness: Witness::new(),
        };

        let recipient_output = TxOut {
            value: Amount::from_sat(amount_sat),
            script_pubkey: ScriptBuf::new(),
        };

        let mut outputs = vec![recipient_output];

        // Add change output if needed
        if change_sat > 0 {
            outputs.push(TxOut {
                value: Amount::from_sat(change_sat),
                script_pubkey: ScriptBuf::new(),
            });
        }

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![input],
            output: outputs,
        };

        let sighash = sha256::Hash::hash(&serialize(&tx));
        Ok((tx, sighash.to_byte_array()))
    }
}

#[derive(Debug)]
pub struct PendingSpend {
    pub tx: Transaction,
}

impl NodeState {
    pub fn get_frost_public_key(&self) -> Option<String> {
        self.pubkey_package.as_ref().map(|p| format!("{:?}", p.verifying_key()).replace("VerifyingKey(", "").replace(")", "").replace("\\", "").replace("\"", ""))
    }

    pub fn frost_signature_to_bitcoin(
        frost_sig: &frost::Signature,
    ) -> Result<bitcoin::secp256k1::schnorr::Signature, String> {
        let sig_bytes = frost_sig
            .serialize()
            .map_err(|e| format!("Serialize frost sig: {}", e))?;

        let schnorr_bytes = match sig_bytes.len() {
            64 => sig_bytes,
            65 => sig_bytes[..64].to_vec(),
            _ => return Err(format!("Unsupported signature len {}", sig_bytes.len())),
        };

        bitcoin::secp256k1::schnorr::Signature::from_slice(&schnorr_bytes)
            .map_err(|e| format!("Parse schnorr sig: {}", e))
    }

    pub fn start_spend_request(&mut self, amount_sat: u64) -> Option<String> {
        println!("🚀 Creating spend request for {} sat", amount_sat);
        match self.wallet.create_spend(amount_sat) {
            Ok((tx, sighash)) => {
                let sighash_hex = hex::encode(sighash);
                self.start_signing_session(&sighash_hex);

                if let Some(active) = &self.active_signing {
                    self.pending_spends
                        .insert(active.sign_id, crate::wallet::PendingSpend { tx });
                    println!("🚀 Spend request prepared (session id {})", active.sign_id);

                    Some(sighash.to_lower_hex_string())
                } else {
                    println!("❌ Failed to start signing session");
                    None
                }
            }
            Err(e) => {
                println!("❌ Failed to create spend transaction: {}", e);
                None
            },
        }
    }
}