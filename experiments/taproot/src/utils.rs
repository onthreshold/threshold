use bitcoin::{
    Address, Amount, ScriptBuf, Txid,
    absolute::LockTime,
    hashes::Hash,
    transaction::{OutPoint, Transaction, TxIn, TxOut},
    witness::Witness,
};
use std::str::FromStr;

use crate::wallet::Utxo;

/// Create a mock transaction that pays to the given address
pub fn create_mock_transaction(
    address: &Address,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let prev_txid = Txid::from_slice(&[1u8; 32])?;

    let input = TxIn {
        previous_output: OutPoint {
            txid: prev_txid,
            vout: 0,
        },
        script_sig: ScriptBuf::new(),
        sequence: bitcoin::Sequence::ZERO,
        witness: Witness::new(),
    };

    let taproot_output = TxOut {
        value: Amount::from_sat(100_000),
        script_pubkey: address.script_pubkey(),
    };

    let change_address =
        Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")?.assume_checked();
    let change_output = TxOut {
        value: Amount::from_sat(50_000),
        script_pubkey: change_address.script_pubkey(),
    };

    Ok(Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![input],
        output: vec![taproot_output, change_output],
    })
}

/// Create UTXO from transaction
pub fn create_utxo(
    tx: &Transaction,
    output_index: u32,
) -> Result<Utxo, Box<dyn std::error::Error>> {
    if output_index as usize >= tx.output.len() {
        return Err("Output index out of bounds".into());
    }

    Ok(Utxo {
        outpoint: OutPoint {
            txid: tx.compute_txid(),
            vout: output_index,
        },
        output: tx.output[output_index as usize].clone(),
        block_height: Some(800_000),
    })
}
