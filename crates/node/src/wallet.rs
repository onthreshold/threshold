use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::{OutPoint, Version};
use bitcoin::witness::Witness;
use bitcoin::{Amount, ScriptBuf, Transaction, TxIn, TxOut};

use crate::db::Db;
use crate::{Network, NodeState};

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
#[derive(Debug, Default)]
pub struct SimpleWallet {
    pub utxos: Vec<Utxo>,
    pub address: Option<bitcoin::Address>,
}

impl SimpleWallet {
    pub fn new(address: &bitcoin::Address) -> Self {
        Self {
            address: Some(address.clone()),
            utxos: vec![
                Utxo {
                    outpoint: OutPoint {
                        txid: bitcoin::Txid::from_slice(&[0u8; 32]).expect("Failed to create UTXO"),
                        vout: 0,
                    },
                    value: Amount::from_sat(100_000),
                    script_pubkey: address.script_pubkey(),
                },
                Utxo {
                    outpoint: OutPoint {
                        txid: bitcoin::Txid::from_slice(&[1u8; 32]).expect("Failed to create UTXO"),
                        vout: 0,
                    },
                    value: Amount::from_sat(50_000),
                    script_pubkey: address.script_pubkey(),
                },
                Utxo {
                    outpoint: OutPoint {
                        txid: bitcoin::Txid::from_slice(&[2u8; 32]).expect("Failed to create UTXO"),
                        vout: 0,
                    },
                    value: Amount::from_sat(20_000),
                    script_pubkey: address.script_pubkey(),
                },
            ],
        }
    }

    pub fn create_spend(
        &mut self,
        amount_sat: u64,
        address: &bitcoin::Address,
    ) -> Result<(Transaction, [u8; 32]), String> {
        let estimated_fee_sat = 200u64;

        let total_needed = amount_sat + estimated_fee_sat;

        let pos = self
            .utxos
            .iter()
            .position(|u| u.value.to_sat() >= total_needed)
            .ok_or_else(|| {
                format!("No single UTXO large enough – need {} sats (amount: {} + fee: {}), coin selection not implemented", 
                        total_needed, amount_sat, estimated_fee_sat)
            })?;

        let utxo = self.utxos.remove(pos);
        let change_sat = utxo.value.to_sat() - amount_sat - estimated_fee_sat;

        let input = TxIn {
            previous_output: utxo.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::ZERO,
            witness: Witness::new(),
        };

        let recipient_output = TxOut {
            value: Amount::from_sat(amount_sat),
            script_pubkey: address.script_pubkey(),
        };

        let mut outputs = vec![recipient_output];

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
            .map_err(|e| format!("Failed to calculate sighash: {}", e))?;

        Ok((tx, sighash.to_byte_array()))
    }
}

#[derive(Debug)]
pub struct PendingSpend {
    pub tx: Transaction,
}

impl<N: Network, D: Db> NodeState<N, D> {
    pub fn get_frost_public_key(&self) -> Option<String> {
        self.pubkey_package.as_ref().map(|p| {
            format!("{:?}", p.verifying_key())
                .replace("VerifyingKey(", "")
                .replace(")", "")
                .replace("\\", "")
                .replace("\"", "")
        })
    }
}
