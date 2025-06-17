pub mod taproot;

#[cfg(test)]
mod utxo_spend_test {
    use node::key_manager::generate_keys_from_mnemonic;
    use node::wallet::TaprootWallet;
    use node::wallet::Wallet;
    use oracle::esplora::EsploraOracle;
    use oracle::oracle::Oracle;

    #[ignore]
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

        let (address, private_key, _) = generate_keys_from_mnemonic(&mnemonic);
        let (address_to, private_key_to, _) = generate_keys_from_mnemonic(&mnemonic_to);

        let oracle = EsploraOracle::new(
            bitcoin::network::Network::Testnet,
            Some(100),
            None,
            None,
            6,
            0,
        );

        let mut wallet_one = TaprootWallet::new(
            Box::new(oracle.clone()),
            vec![address.clone()],
            bitcoin::network::Network::Testnet,
        );
        let mut wallet_two = TaprootWallet::new(
            Box::new(oracle.clone()),
            vec![address_to.clone()],
            bitcoin::network::Network::Testnet,
        );

        wallet_one
            .refresh_utxos(Some(true))
            .await
            .expect("Failed to refresh utxos");
        wallet_two
            .refresh_utxos(Some(true))
            .await
            .expect("Failed to refresh utxos");

        let result = wallet_one.create_spend(1000, 200, &address_to, false);

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
                let result = wallet_two.create_spend(amount - fee, fee, &address, false);
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
