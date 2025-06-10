use bitcoin::absolute::LockTime;
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::transaction::{OutPoint, Version};
use bitcoin::witness::Witness;
use bitcoin::{Amount, ScriptBuf, Transaction, TxIn, TxOut, hashes::sha256};

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
            script_pubkey: address.script_pubkey(),
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
