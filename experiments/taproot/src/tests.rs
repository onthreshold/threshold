#[cfg(test)]
mod tests {
    use bitcoin::{Address, Amount};
    use std::str::FromStr;

    use crate::utils::{create_mock_transaction, create_utxo};
    use crate::wallet::FrostTaprootWallet;

    #[test]
    fn test_wallet_creation() {
        let wallet = FrostTaprootWallet::new(3, 5).unwrap();
        assert_eq!(wallet.min_signers, 3);
        assert_eq!(wallet.max_signers, 5);
        assert_eq!(wallet.participants().len(), 5);
    }

    #[test]
    fn test_different_thresholds() {
        // Test 2-of-3
        let wallet_2_3 = FrostTaprootWallet::new(2, 3).unwrap();
        assert_eq!(wallet_2_3.participants().len(), 3);

        // Test 5-of-7
        let wallet_5_7 = FrostTaprootWallet::new(5, 7).unwrap();
        assert_eq!(wallet_5_7.participants().len(), 7);
    }

    #[test]
    fn test_frost_round_1() {
        let wallet = FrostTaprootWallet::new(3, 5).unwrap();
        let participants = wallet.participants();

        // Try with only 2 participants (less than threshold of 3)
        let result = wallet.frost_round_1(&participants[0..2]);
        assert!(result.is_err());

        // Try with 3 participants (exactly threshold)
        let result = wallet.frost_round_1(&participants[0..3]);
        assert!(result.is_ok());

        let (nonces, commitments) = result.unwrap();
        assert_eq!(nonces.len(), 3);
        assert_eq!(commitments.len(), 3);
    }

    #[test]
    fn test_utxo_creation() {
        let wallet = FrostTaprootWallet::new(3, 5).unwrap();
        let tx = create_mock_transaction(wallet.address()).unwrap();
        let utxo = create_utxo(&tx, 0).unwrap();

        assert!(utxo.output.script_pubkey.is_p2tr());
        assert_eq!(utxo.output.value, Amount::from_sat(100_000));
    }

    #[test]
    fn test_complete_signing_workflow() {
        let wallet = FrostTaprootWallet::new(3, 5).unwrap();

        // Create mock UTXO
        let mock_tx = create_mock_transaction(wallet.address()).unwrap();
        let utxo = create_utxo(&mock_tx, 0).unwrap();

        // Set up signing
        let participants = wallet.participants();
        let signing_participants = participants.into_iter().take(3).collect();
        let recipient = Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")
            .unwrap()
            .assume_checked();

        // Attempt signing
        let result = wallet.sign_transaction(
            &utxo,
            &recipient,
            Amount::from_sat(90_000),
            signing_participants,
        );

        // Should succeed with the official FROST library
        assert!(result.is_ok());

        let signed_tx = result.unwrap();
        assert_eq!(signed_tx.input.len(), 1);
        assert_eq!(signed_tx.output.len(), 1);
        assert!(!signed_tx.input[0].witness.is_empty());
    }

    #[test]
    fn test_dkg_deterministic_group_key() {
        // All participants should derive the same group public key from DKG
        let wallet1 = FrostTaprootWallet::new(2, 3).unwrap();
        let wallet2 = FrostTaprootWallet::new(2, 3).unwrap();

        // Different DKG runs should produce different group keys
        assert_ne!(
            wallet1.pubkey_package.verifying_key().serialize().unwrap(),
            wallet2.pubkey_package.verifying_key().serialize().unwrap()
        );

        // But within same DKG run, all participants have same group key
        // (This is verified in the DKG module during generation)
    }

    #[test]
    fn test_insufficient_signers() {
        let wallet = FrostTaprootWallet::new(3, 5).unwrap();
        let participants = wallet.participants();

        // Try to sign with only 2 participants (below threshold of 3)
        let mock_tx = create_mock_transaction(wallet.address()).unwrap();
        let utxo = create_utxo(&mock_tx, 0).unwrap();
        let recipient = Address::from_str("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")
            .unwrap()
            .assume_checked();

        let insufficient_participants = participants.into_iter().take(2).collect();

        let result = wallet.sign_transaction(
            &utxo,
            &recipient,
            Amount::from_sat(90_000),
            insufficient_participants,
        );

        // Should fail due to insufficient participants
        assert!(result.is_err());
    }
}
