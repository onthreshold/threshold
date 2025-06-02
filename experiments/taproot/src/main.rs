use std::str::FromStr;

use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Transaction, TxIn, TxOut, Txid, Witness,
    absolute::LockTime,
    hashes::Hash,
    secp256k1::{Secp256k1, SecretKey, XOnlyPublicKey, rand},
    taproot::TaprootSpendInfo,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize secp256k1 context
    let secp = Secp256k1::new();

    // Generate a random private key for our internal key
    let internal_key = SecretKey::new(&mut rand::thread_rng());
    let internal_pubkey = XOnlyPublicKey::from(internal_key.public_key(&secp));

    println!("Internal Public Key: {}", internal_pubkey);

    // Example 2: Script-path Taproot with simple scripts
    println!("\n=== Taproot Address ===");
    let script_address = create_taproot_address(&secp, internal_pubkey)?;
    println!("Address: {}", script_address);

    println!("\n=== Mock Transaction ===");
    let mock_transaction = create_mock_transaction_to_taproot(&script_address)?;
    println!("Transaction: {:?}", mock_transaction);

    println!("\n=== UTXO ===");
    let utxo = create_utxo_from_transaction(&mock_transaction, 0)?;
    println!("UTXO: {:?}", utxo);

    Ok(())
}

/// Creates a Taproot address with simple script paths
fn create_taproot_address(
    secp: &Secp256k1<bitcoin::secp256k1::All>,
    internal_key: XOnlyPublicKey,
) -> Result<Address, Box<dyn std::error::Error>> {
    let taproot_spend_info = TaprootSpendInfo::new_key_spend(secp, internal_key, None);

    let output_key = taproot_spend_info.output_key();

    let address: Address = Address::p2tr(secp, internal_key, None, Network::Bitcoin);

    println!("Output Key: {}", output_key);

    Ok(address)
}

fn create_mock_transaction_to_taproot(
    taproot_address: &Address,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let prev_txid = Txid::from_slice(&[1u8; 32])?;

    let input = TxIn {
        previous_output: OutPoint {
            txid: prev_txid,
            vout: 0,
        },
        script_sig: ScriptBuf::new(), // Empty for Taproot (witness-based)
        sequence: bitcoin::Sequence::ZERO,
        witness: Witness::new(), // Would contain actual witness data
    };

    // Create output that pays to our Taproot address
    let taproot_output = TxOut {
        value: Amount::from_sat(100_000), // 0.001 BTC = 100,000 sats
        script_pubkey: taproot_address.script_pubkey(),
    };

    // Create a change output (paying back to sender)
    let change_address =
        Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")?.assume_checked(); // Mock P2WPKH address
    let change_output = TxOut {
        value: Amount::from_sat(50_000), // 0.0005 BTC change
        script_pubkey: change_address.script_pubkey(),
    };

    // Construct the transaction
    let transaction = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![input],
        output: vec![taproot_output, change_output],
    };

    Ok(transaction)
}

#[derive(Debug)]
struct Utxo {
    outpoint: OutPoint,
    output: TxOut,
    block_height: Option<u32>, // When it was confirmed
}

fn create_utxo_from_transaction(
    tx: &Transaction,
    output_index: u32,
) -> Result<Utxo, Box<dyn std::error::Error>> {
    if output_index as usize >= tx.output.len() {
        return Err("Output index out of bounds".into());
    }

    let utxo = Utxo {
        outpoint: OutPoint {
            txid: tx.compute_txid(),
            vout: output_index,
        },
        output: tx.output[output_index as usize].clone(),
        block_height: Some(800_000), // Mock block height
    };

    Ok(utxo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taproot_creation() {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        let internal_key = SecretKey::new(&mut rng);
        let internal_pubkey = XOnlyPublicKey::from(internal_key.public_key(&secp));

        // Test script Taproot
        let script_taproot = create_taproot_address(&secp, internal_pubkey);
        dbg!(&script_taproot);
        assert!(script_taproot.is_ok());
    }

    #[test]
    fn test_utxo_creation() {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        let internal_key = SecretKey::new(&mut rng);
        let internal_pubkey = XOnlyPublicKey::from(internal_key.public_key(&secp));

        let address = Address::p2tr(&secp, internal_pubkey, None, Network::Bitcoin);
        let tx = create_mock_transaction_to_taproot(&address).unwrap();
        let utxo = create_utxo_from_transaction(&tx, 0).unwrap();
        dbg!(&utxo);

        assert!(utxo.output.script_pubkey.is_p2tr());
        assert_eq!(utxo.output.value, Amount::from_sat(100_000));
    }
}
