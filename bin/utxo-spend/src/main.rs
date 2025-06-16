use bitcoin::Address;
use node::key_manager::generate_keys_from_mnemonic;
use node::wallet::{TaprootWallet, Wallet};
use oracle::esplora::EsploraOracle;
use oracle::oracle::Oracle;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Get command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        println!(
            "Usage: {} <destination_address> <amount_in_sats> [fee_in_sats]",
            args[0]
        );
        std::process::exit(1);
    }

    let address_to = Address::from_str(&args[1])
        .expect("Invalid Bitcoin address")
        .assume_checked();
    let amount = args[2]
        .parse::<u64>()
        .expect("Amount must be a valid number");

    let fee = args[3]
        .parse::<u64>()
        .expect("Fee sats must be a valid number");

    let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC env variable not set");
    let (address, private_key, compressed_public_key) =
        generate_keys_from_mnemonic(mnemonic.as_str());
    println!("Sender address: {}. Loading wallet utxos...", address);

    let oracle = EsploraOracle::new(
        bitcoin::network::Network::Testnet,
        Some(100),
        None,
        None,
        6,
        -1,
    );
    let mut wallet = TaprootWallet::new(
        Box::new(oracle.clone()),
        vec![address],
        bitcoin::network::Network::Testnet,
    );

    wallet.refresh_utxos(Some(true)).await.unwrap();

    let (tx, sighash) = wallet
        .create_spend(amount, fee, &address_to, false)
        .unwrap();
    println!(
        "Created Transaction for amount: {} to address: {}",
        amount, address_to
    );
    let tx_id = tx.compute_txid();

    let signed_tx = wallet.sign(&tx, &private_key, sighash);

    println!("Public key: {:?}", compressed_public_key);
    println!("Signed transaction");

    oracle
        .broadcast_transaction(&signed_tx)
        .await
        .expect("Failed to broadcast transaction");

    println!("Broadcast Transaction txid: {:?}", tx_id);
}
