#[cfg(test)]
mod taproot_wallet_tests {
    use crate::mocks::pubkey::random_public_key;
    use bitcoin::secp256k1::Scalar;
    use bitcoin::{Amount, Network};
    use node::wallet::TaprootWallet;
    use node::wallet::Wallet;
    use oracle::mock::MockOracle;
    use protocol::block::{Block, BlockBody, BlockHeader};
    use protocol::transaction::{Transaction, TransactionType};
    use serde_json::json;
    use tokio::sync::broadcast;
    use types::network::network_event::NetworkEvent;

    fn create_test_wallet() -> TaprootWallet {
        let (tx_channel, _) = broadcast::channel::<NetworkEvent>(100);
        let oracle = MockOracle::new(tx_channel, None);
        TaprootWallet::new(Box::new(oracle), Vec::new(), Network::Testnet)
    }

    fn create_withdrawal_transaction(address: &str, amount_sat: u64) -> Transaction {
        Transaction::new(
            TransactionType::Withdrawal,
            vec![],
            Some(json!({
                "address_to": address,
                "amount_sat": amount_sat
            })),
        )
    }

    fn create_test_block(transactions: Vec<Transaction>) -> Block {
        Block {
            header: BlockHeader {
                version: 1,
                previous_block_hash: [0u8; 32],
                state_root: [0u8; 32],
                height: 1,
                proposer: vec![],
            },
            body: BlockBody { transactions },
        }
    }

    #[tokio::test]
    async fn test_taproot_wallet_create_and_refresh() {
        let (tx_channel, _) = broadcast::channel::<NetworkEvent>(100);
        let oracle = MockOracle::new(tx_channel, None);

        // Create an empty Taproot wallet on testnet
        let mut wallet = TaprootWallet::new(Box::new(oracle.clone()), Vec::new(), Network::Testnet);
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

    #[tokio::test]
    async fn test_get_transaction_for_block_single_withdrawal() {
        let mut wallet = create_test_wallet();
        let pubkey = random_public_key();
        let tweak = Scalar::from_be_bytes([1u8; 32]).unwrap();

        // Setup wallet with address and UTXOs
        wallet.generate_new_address(pubkey, tweak);
        wallet
            .refresh_utxos(Some(true))
            .await
            .expect("refresh_utxos failed");

        // Create a block with a single withdrawal
        let withdrawal_tx =
            create_withdrawal_transaction("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", 50000);
        let block = create_test_block(vec![withdrawal_tx]);

        // Test transaction creation
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_ok());

        let bitcoin_tx = result.unwrap();
        assert!(!bitcoin_tx.input.is_empty());
        assert_eq!(bitcoin_tx.output.len(), 2); // 1 payout + 1 change
        assert_eq!(bitcoin_tx.output[0].value, Amount::from_sat(50000));
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_multiple_withdrawals() {
        let mut wallet = create_test_wallet();
        let pubkey = random_public_key();
        let tweak = Scalar::from_be_bytes([1u8; 32]).unwrap();

        // Setup wallet with address and UTXOs
        wallet.generate_new_address(pubkey, tweak);
        wallet
            .refresh_utxos(Some(true))
            .await
            .expect("refresh_utxos failed");

        // Create a block with multiple withdrawals
        let withdrawal_tx1 =
            create_withdrawal_transaction("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", 25000);
        let withdrawal_tx2 = create_withdrawal_transaction(
            "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
            30000,
        );
        let block = create_test_block(vec![withdrawal_tx1, withdrawal_tx2]);

        // Test transaction creation
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_ok());

        let bitcoin_tx = result.unwrap();
        assert!(!bitcoin_tx.input.is_empty());
        assert_eq!(bitcoin_tx.output.len(), 3); // 2 payouts + 1 change
        assert_eq!(bitcoin_tx.output[0].value, Amount::from_sat(25000));
        assert_eq!(bitcoin_tx.output[1].value, Amount::from_sat(30000));
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_no_withdrawals() {
        let wallet = create_test_wallet();

        // Create a block with no withdrawal transactions
        let deposit_tx = Transaction::new(TransactionType::Deposit, vec![], None);
        let block = create_test_block(vec![deposit_tx]);

        // Test should return error for no withdrawals
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no withdrawals"));
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_missing_metadata() {
        let wallet = create_test_wallet();

        // Create a withdrawal transaction with missing metadata
        let withdrawal_tx = Transaction::new(TransactionType::Withdrawal, vec![], None);
        let block = create_test_block(vec![withdrawal_tx]);

        // Test should return error for missing metadata
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing metadata"));
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_missing_address() {
        let wallet = create_test_wallet();

        // Create a withdrawal transaction with missing address_to
        let withdrawal_tx = Transaction::new(
            TransactionType::Withdrawal,
            vec![],
            Some(json!({ "amount_sat": 50000 })),
        );
        let block = create_test_block(vec![withdrawal_tx]);

        // Test should return error for missing address
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing address_to")
        );
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_missing_amount() {
        let wallet = create_test_wallet();

        // Create a withdrawal transaction with missing amount_sat
        let withdrawal_tx = Transaction::new(
            TransactionType::Withdrawal,
            vec![],
            Some(json!({ "address_to": "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4" })),
        );
        let block = create_test_block(vec![withdrawal_tx]);

        // Test should return error for missing amount
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing amount_sat")
        );
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_insufficient_funds() {
        let wallet = create_test_wallet();

        // Create a withdrawal for a large amount without UTXOs
        let withdrawal_tx = create_withdrawal_transaction(
            "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
            1_000_000_000,
        );
        let block = create_test_block(vec![withdrawal_tx]);

        // Test should return error for insufficient funds
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no spendable UTXOs")
        );
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_invalid_address() {
        let wallet = create_test_wallet();

        // Create a withdrawal transaction with invalid address
        let withdrawal_tx = Transaction::new(
            TransactionType::Withdrawal,
            vec![],
            Some(json!({
                "address_to": "invalid_address",
                "amount_sat": 50000
            })),
        );
        let block = create_test_block(vec![withdrawal_tx]);

        // Test should return error for invalid address
        let result = wallet.get_transaction_for_block(block, 10);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_dust_change() {
        let mut wallet = create_test_wallet();
        let pubkey = random_public_key();
        let tweak = Scalar::from_be_bytes([1u8; 32]).unwrap();

        // Setup wallet with address and UTXOs
        wallet.generate_new_address(pubkey, tweak);
        wallet
            .refresh_utxos(Some(true))
            .await
            .expect("refresh_utxos failed");

        let withdrawal_tx =
            create_withdrawal_transaction("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", 500);
        let block = create_test_block(vec![withdrawal_tx]);

        // Test transaction creation with very low fee rate to maximize change
        let result = wallet.get_transaction_for_block(block, 1); // Low fee rate    
        assert!(result.is_ok());

        let bitcoin_tx = result.unwrap();
        assert_eq!(bitcoin_tx.output[0].value, Amount::from_sat(500));

        assert!(!bitcoin_tx.output.is_empty());
        assert_eq!(bitcoin_tx.output.len(), 2);
    }

    #[tokio::test]
    async fn test_get_transaction_for_block_high_fee_rate() {
        let mut wallet = create_test_wallet();
        let pubkey = random_public_key();
        let tweak = Scalar::from_be_bytes([1u8; 32]).unwrap();

        // Setup wallet with address and UTXOs
        wallet.generate_new_address(pubkey, tweak);
        wallet
            .refresh_utxos(Some(true))
            .await
            .expect("refresh_utxos failed");

        // Create a withdrawal with high fee rate
        let withdrawal_tx =
            create_withdrawal_transaction("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", 50000);
        let block = create_test_block(vec![withdrawal_tx]);

        // Test transaction creation with high fee rate
        let result = wallet.get_transaction_for_block(block, 100); // High fee rate
        assert!(result.is_ok());

        let bitcoin_tx = result.unwrap();
        assert!(!bitcoin_tx.input.is_empty());
        assert_eq!(bitcoin_tx.output[0].value, Amount::from_sat(50000));

        // With high fee rate, change should be significantly less
        if bitcoin_tx.output.len() == 2 {
            assert!(bitcoin_tx.output[1].value.to_sat() < 40000); // Less than low fee case
        }
    }
}
