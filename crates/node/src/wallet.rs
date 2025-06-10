use crate::db::Db;
use crate::{Network, NodeState};
use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::witness::Witness;
use bitcoin::{Address, OutPoint, Txid};
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
        let client_utxos = refresh_utxos(&esplora_client.client, address.clone(), 3, None)
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
            .map_err(|e| format!("Failed to calculate sighash: {}", e))?;

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

pub async fn broadcast_transaction(tx: &Transaction) -> Result<String, NodeError> {
    // Create esplora client for testnet4
    let builder = esplora_client::Builder::new("https://blockstream.info/testnet/api");
    let client = builder.build_async().unwrap();

    // Serialize the transaction to raw bytes
    let tx_bytes = bitcoin::consensus::encode::serialize(tx);
    let tx_hex = hex::encode(&tx_bytes);

    // Broadcast the transaction
    client
        .broadcast(tx)
        .await
        .map_err(|e| NodeError::Error(format!("Failed to broadcast transaction: {}", e)))?;

    Ok(tx_hex)
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
    let script = address.script_pubkey();

    for _ in 0..number_pages {
        let address_txs = client
            .scripthash_txs(&script, last_seen_txid)
            .await
            .map_err(|e| {
                NodeError::Error(format!("Cannot retrieve transactions for address: {}", e))
            })?;

        if address_txs.is_empty() {
            break;
        }

        last_seen_txid = Some(address_txs.last().unwrap().txid);
        for tx in address_txs {
            let Some(full_tx) = client.get_tx(&tx.txid).await.ok().flatten() else {
                continue;
            };
            let Ok(tx_status) = client.get_tx_status(&tx.txid).await else {
                continue;
            };
            if !tx_status.confirmed {
                continue;
            }

            for (vout, output) in full_tx.output.iter().enumerate() {
                if output.script_pubkey != script {
                    continue;
                }
                let Ok(Some(output_status)) = client.get_output_status(&tx.txid, vout as u64).await
                else {
                    continue;
                };
                if output_status.spent {
                    continue;
                }
                unspent_txs.push(Utxo {
                    outpoint: OutPoint {
                        txid: tx.txid,
                        vout: vout as u32,
                    },
                    value: Amount::from_sat(output.value.to_sat()),
                    script_pubkey: script.clone(),
                });
            }
        }

        if last_seen_txid.is_none() {
            break;
        }
    }

    Ok(unspent_txs)
}
