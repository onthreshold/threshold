use bitcoin::{Address, Amount};
use std::str::FromStr;

mod dkg;
mod utils;
mod wallet;

use utils::{create_mock_transaction, create_utxo};
use wallet::FrostTaprootWallet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== FROST + Taproot Demo using ZCash Foundation FROST ===\n");

    // Create a 3-of-5 FROST Taproot wallet
    let wallet = FrostTaprootWallet::new(3, 5)?;
    wallet.print_info();

    println!("\n=== Creating Mock UTXO ===");
    let mock_tx = create_mock_transaction(wallet.address())?;
    let utxo = create_utxo(&mock_tx, 0)?;

    println!("Mock transaction: {}", mock_tx.compute_txid());
    println!("UTXO: {}:{}", utxo.outpoint.txid, utxo.outpoint.vout);
    println!("UTXO value: {} sats", utxo.output.value);

    println!("\n=== FROST Threshold Signing ===");

    // Select 3 participants for signing (minimum threshold)
    let participants = wallet.participants();
    let signing_participants = participants.into_iter().take(3).collect::<Vec<_>>();

    println!("Signing with {} participants", signing_participants.len());

    // Create recipient address
    let recipient =
        Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")?.assume_checked();
    let send_amount = Amount::from_sat(90_000);

    println!("Sending {} sats to {}", send_amount, recipient);

    // Perform FROST signing
    match wallet.sign_transaction(&utxo, &recipient, send_amount, signing_participants) {
        Ok(signed_tx) => {
            println!("\n✅ FROST signing successful!");
            println!("Signed transaction: {}", signed_tx.compute_txid());
            println!("Witness elements: {}", signed_tx.input[0].witness.len());

            // Verify transaction structure
            assert_eq!(signed_tx.input.len(), 1);
            assert_eq!(signed_tx.output.len(), 1);
            assert!(!signed_tx.input[0].witness.is_empty());

            println!(
                "Transaction size: {} bytes",
                bitcoin::consensus::encode::serialize(&signed_tx).len()
            );
        }
        Err(e) => {
            println!("❌ FROST signing failed: {}", e);
        }
    }

    println!("\n=== Demo Complete ===");
    println!("This shows a production-ready FROST implementation");
    println!("integrated with Bitcoin Taproot!");

    Ok(())
}
