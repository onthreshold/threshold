#[cfg(test)]
mod utxo_spend_test {
    use node::key_manager::generate_keys_from_mnemonic;
    use node::wallet::SimpleWallet;
    use protocol::oracle::EsploraOracle;
    use protocol::oracle::Oracle;

    #[tokio::test]
    pub async fn test_utxo_spend() {
        let amount = 1000;
        let fee = 200;

        dotenvy::dotenv().ok();
        let mnemonic = std::env::var("MNEMONIC").unwrap_or_else(|_| {
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string()
        });

        let mnemonic_to = std::env::var("MNEMONIC_TO").unwrap_or_else(|_| {
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string()
        });

        let (address, private_key) = generate_keys_from_mnemonic(&mnemonic);
        let (address_to, private_key_to) = generate_keys_from_mnemonic(&mnemonic_to);

        let oracle = EsploraOracle::new(true);

        let mut wallet_one = SimpleWallet::new(&address, oracle.clone(), Some(true)).await;
        let mut wallet_two = SimpleWallet::new(&address_to, oracle.clone(), Some(true)).await;

        let result = wallet_one.create_spend(1000, 200, &address_to);

        match result {
            Ok((tx, sighash)) => {
                let signed_tx = wallet_one.sign(&tx, &private_key, sighash);
                oracle
                    .broadcast_transaction(&signed_tx)
                    .await
                    .expect("Failed to broadcast transaction");
                println!("Transaction created and signed successfully");

                // wait for the transaction to be confirmed
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                // return money
                let result = wallet_two.create_spend(amount - fee, fee, &address);
                match result {
                    Ok((tx, sighash)) => {
                        let signed_tx = wallet_two.sign(&tx, &private_key_to, sighash);
                        oracle
                            .broadcast_transaction(&signed_tx)
                            .await
                            .expect("Failed to broadcast transaction");
                        println!("Transaction created and signed successfully");
                    }
                    Err(e) => {
                        panic!("Transaction creation failed: {:?}", e);
                    }
                }
            }
            Err(e) => {
                panic!("Transaction creation failed: {:?}", e);
            }
        }
    }
}
