use crate::db::Db;
use crate::{Network, NodeState};
use bitcoin::{Address, OutPoint, Txid};
use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::witness::Witness;
use bitcoin::{Amount, ScriptBuf, Transaction, TxIn, TxOut};
use clients::{EsploraApiClient, NodeError};
use esplora_client::AsyncClient;

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
    pub async fn new(address: &bitcoin::Address) -> Self {
        let esplora_client = EsploraApiClient::default();
        let client_utxos = refresh_utxos(&esplora_client.client, address.clone(), 3, None).await.unwrap();

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


async fn refresh_utxos(
    client: &AsyncClient,
    address: Address,
    number_pages: u32,
    start_transactions: Option<Txid>,
) -> Result<Vec<Utxo>, NodeError> {
    let mut unspent_txs = Vec::new();
    let mut last_seen_txid = start_transactions;

    for _ in 0..number_pages {
        let address_txs = client
            .scripthash_txs(&address.script_pubkey(), last_seen_txid)
            .await
            .map_err(|e| {
                NodeError::Error(format!("Cannot retrieve transactions for address: {}", e))
            })?;

        if address_txs.is_empty() {
            break;
        }

        last_seen_txid = Some(address_txs.last().unwrap().txid);

        for tx in address_txs {
            if let Ok(Some(full_tx)) = client.get_tx(&tx.txid).await {
                for (vout, output) in full_tx.output.iter().enumerate() {
                    if output.script_pubkey == address.script_pubkey() {
                        let outpoint = OutPoint {
                            txid: tx.txid,
                            vout: vout as u32,
                        };

                        if let Ok(tx_status) = client.get_tx_status(&tx.txid).await {
                            if tx_status.confirmed {
                                if let Ok(Some(output_status)) =
                                    client.get_output_status(&tx.txid, vout as u64).await
                                {
                                    if !output_status.spent {
                                        unspent_txs.push(Utxo {
                                            outpoint,
                                            value: Amount::from_sat(output.value.to_sat()),
                                            script_pubkey: address.script_pubkey(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if last_seen_txid.is_none() {
            break;
        }
    }

    Ok(unspent_txs)
}