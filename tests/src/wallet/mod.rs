#[cfg(test)]
mod utxo_spend_test {
    use bitcoin::Address;
    use std::str::FromStr;

    use node::key_manager::generate_keys_from_mnemonic;
    use node::wallet::SimpleWallet;

    #[tokio::test]
    pub async fn test_utxo_spend() {
        dotenvy::dotenv().ok();
        let mnemonic = std::env::var("MNEMONIC").unwrap_or_else(|_| {
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string()
        });

        let (address, private_key) = generate_keys_from_mnemonic(&mnemonic);
        println!("Using address from mnemonic: {}", address);

        let address_to =
            Address::from_str("bc1pg520trs6vk0yplnx0mp5v7lrx2yyw00nqy8qmzdpevpz3knjz9fq2ps3jx")
                .unwrap()
                .assume_checked();

        let mut wallet = SimpleWallet::new(&address).await;

        let result = wallet.create_spend(1000, 100, &address_to);

        match result {
            Ok((tx, sighash)) => {
                let signed_tx = wallet.sign(&tx, &private_key, sighash);
                node::wallet::SimpleWallet::broadcast_transaction(&signed_tx)
                    .await
                    .expect("Failed to broadcast transaction");
                println!("Transaction created and signed successfully");
            }
            Err(e) => {
                panic!("Transaction creation failed: {:?}", e);
            }
        }
    }
}
