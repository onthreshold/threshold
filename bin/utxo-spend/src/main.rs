use bip39::{Language, Mnemonic};
#[allow(deprecated)]
use bitcoin::bip32::{DerivationPath, ExtendedPrivKey};
use bitcoin::consensus::encode::serialize;
use bitcoin::key::Secp256k1;
use bitcoin::transaction::OutPoint;
use bitcoin::witness::Witness;
use bitcoin::{Address, Amount, CompressedPublicKey, Network, PrivateKey, Transaction, Txid};
use esplora_client::Builder;
use node::wallet::SimpleWallet;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC env variable not set");
    let (address, private_key) = generate_from_mnemonic(mnemonic.as_str());
    println!("Address: {}", address);

    let address_to =
        Address::from_str("bc1pg520trs6vk0yplnx0mp5v7lrx2yyw00nqy8qmzdpevpz3knjz9fq2ps3jx")
            .unwrap()
            .assume_checked();

    let mut wallet = SimpleWallet::new(&address).await;

    let (mut tx, sighash) = wallet.create_spend(1000, &address_to).unwrap();
    println!("Tx: {:?}", tx);
    println!("Sighash: {:?}", hex::encode(sighash));

    // Sign the transaction
    sign_transaction(&mut tx, &private_key, &sighash);

    println!("Tx signed: {:?}", tx);

    // // Broadcast the transaction
    broadcast_transaction(&tx).await;
}

fn sign_transaction(tx: &mut Transaction, private_key: &PrivateKey, sighash: &[u8; 32]) {
    // For P2WPKH, we need to create a witness signature
    let secp = Secp256k1::new();

    // Create the signature using the properly calculated sighash
    let message = bitcoin::secp256k1::Message::from_digest(*sighash);
    let signature = secp.sign_ecdsa(&message, &private_key.inner);

    // Create witness with signature + sighash type (0x01 = SIGHASH_ALL)
    let mut sig_bytes = signature.serialize_der().to_vec();
    sig_bytes.push(0x01); // SIGHASH_ALL

    let compressed_pubkey = CompressedPublicKey::from_private_key(&secp, private_key)
        .expect("Failed to get compressed public key");

    let mut witness = Witness::new();
    witness.push(sig_bytes);
    witness.push(compressed_pubkey.to_bytes());

    // Add witness to the first (and only) input
    if let Some(input) = tx.input.first_mut() {
        input.witness = witness;
    }

    println!("Transaction signed!");
}

async fn broadcast_transaction(tx: &Transaction) {
    dotenvy::dotenv().ok();
    let is_testnet: bool = std::env::var("IS_TESTNET")
        .unwrap_or("false".to_string())
        .parse()
        .unwrap();

    // Create esplora client for testnet4
    let builder = Builder::new(if is_testnet {
        "https://blockstream.info/testnet/api"
    } else {
        "https://blockstream.info/api"
    });
    let client = builder.build_async().unwrap();

    // Serialize the transaction to raw bytes
    let tx_bytes = serialize(tx);
    let tx_hex = hex::encode(&tx_bytes);

    println!("Raw transaction: {}", tx_hex);

    // Broadcast the transaction
    match client.broadcast(tx).await {
        Ok(()) => {
            println!("✅ Transaction broadcast successfully!");
            println!("Txid: {:?}", tx.compute_txid());
        }
        Err(e) => {
            println!("❌ Failed to broadcast transaction: {}", e);
        }
    }
}

pub fn get_utxos_for_address(address: &Address) -> Vec<node::wallet::Utxo> {
    let utxos = vec![node::wallet::Utxo {
        outpoint: OutPoint::new(
            Txid::from_str("f0599c6679a42ffd8b45f16e7944f1d73ad72cc4b7193e6d7485587ebd8e3e9d")
                .unwrap(),
            0,
        ),
        value: Amount::from_sat(4_000),
        script_pubkey: address.script_pubkey(),
    }];
    utxos
}

#[allow(deprecated)]
pub fn generate_from_mnemonic(mnemonic: &str) -> (Address, PrivateKey) {
    dotenvy::dotenv().ok();

    // Generate a new mnemonic (12 words)
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic).unwrap();
    println!("Mnemonic: {}", mnemonic);

    // Convert to seed
    let seed = mnemonic.to_seed(""); // Empty passphrase

    let is_testnet: bool = std::env::var("IS_TESTNET")
        .unwrap_or("false".to_string())
        .parse()
        .unwrap();

    let network = if is_testnet {
        Network::Testnet
    } else {
        Network::Bitcoin
    };

    // Create extended private key
    let secp = Secp256k1::new();

    let xprv = ExtendedPrivKey::new_master(network, &seed).unwrap();

    // Derive key at standard path (m/84'/1'/0'/0/0 for signet P2WPKH)
    let derivation_path = DerivationPath::from_str("m/84'/1'/0'/0/0").unwrap();
    let derived_xprv = xprv.derive_priv(&secp, &derivation_path).unwrap();

    // Get the private key
    let private_key = PrivateKey::new(derived_xprv.private_key, network);
    let compressed_public_key: CompressedPublicKey =
        CompressedPublicKey::from_private_key(&secp, &private_key)
            .expect("Failed to convert public key to compressed public key");
    let address = Address::p2wpkh(&compressed_public_key, network);

    println!("Extended Private Key: {}", xprv);
    println!("Derived Private Key (WIF): {}", private_key.to_wif());
    println!("Address: {}", address);
    (address, private_key)
}
