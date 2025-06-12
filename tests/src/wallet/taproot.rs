#[cfg(test)]
mod taproot_wallet_tests {
    use crate::mocks::oracle::MockOracle;
    use crate::mocks::pubkey::random_public_key;
    use bitcoin::Network;
    use bitcoin::secp256k1::Scalar;
    use node::wallet::TaprootWallet;
    use node::wallet::Wallet;

    #[tokio::test]
    async fn test_taproot_wallet_create_and_refresh() {
        // Initialize mock oracle which returns 3 dummy UTXOs per address queried
        let oracle = MockOracle::new();

        // Create an empty Taproot wallet on testnet
        let mut wallet = TaprootWallet::new(oracle.clone(), Vec::new(), Network::Testnet);
        let pubkey = random_public_key();
        let tweak = Scalar::from_be_bytes([2u8; 32]).unwrap();

        // Generate three P2TR addresses
        let _addr1 = wallet.generate_new_address(pubkey, tweak);
        let _addr2 = wallet.generate_new_address(pubkey, tweak);
        let _addr3 = wallet.generate_new_address(pubkey, tweak);

        assert_eq!(wallet.addresses.len(), 3);

        // Refresh UTXOs across all addresses – MockOracle returns 3 outputs per address
        wallet
            .refresh_utxos(Some(true))
            .await
            .expect("refresh_utxos failed");

        assert_eq!(wallet.utxos.len(), 9);
    }
}
