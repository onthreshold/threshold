#[cfg(test)]
mod deposit_tests {
    use std::str::FromStr;

    use crate::mocks::abci::setup_test_account;
    use crate::mocks::network::MockNodeCluster;
    use abci::chain_state::Account;
    use bitcoin::Address;
    use bitcoin::hashes::Hash;
    use grpc::grpc_operator;
    use node::{handlers::deposit::DepositIntentState, wallet::Wallet};
    use tokio::sync::broadcast;
    use tokio::sync::mpsc::unbounded_channel;
    use types::intents::DepositIntent;
    use types::proto::node_proto::{CreateDepositIntentRequest, CreateDepositIntentResponse};
    use uuid::Uuid;

    #[tokio::test]
    async fn deposit_intent_creates_valid_address_and_persists_on_node() {
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let amount_sat = 50_000;
        let (tx, mut rx) = unbounded_channel::<CreateDepositIntentResponse>();
        let network = cluster.networks.get(&node_peer).unwrap().clone();

        tokio::spawn(async move {
            let response = grpc_operator::create_deposit_intent(
                &network,
                CreateDepositIntentRequest {
                    public_key:
                        "020202020202020202020202020202020202020202020202020202020202020202"
                            .to_string(),
                    amount_satoshis: amount_sat,
                },
            )
            .await
            .expect("Failed to create deposit intent");
            tx.send(response).unwrap();
        });

        cluster.run_n_iterations(10).await;
        println!("cluster.run_n_iterations(10).await;");

        let response = rx.recv().await.unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // retrieve the first deposit intent stored
        let intent_opt = match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetDepositIntentByAddress {
                address: response.deposit_address.clone(),
            })
            .await
        {
            Ok(abci::ChainResponse::GetDepositIntentByAddress { intent }) => intent,
            _ => None,
        };

        println!("intent_opt: {:?}", intent_opt);

        assert!(intent_opt.is_some(), "deposit intent not stored");

        let intent = intent_opt.unwrap();
        assert_eq!(
            intent.user_pubkey,
            "020202020202020202020202020202020202020202020202020202020202020202"
        );

        // parse address and validate
        let addr = Address::from_str(&intent.deposit_address).unwrap();

        // The MockNodeCluster uses Testnet, so validate against Testnet
        assert!(addr.is_valid_for_network(bitcoin::Network::Testnet));
    }

    #[tokio::test]
    async fn deposit_intent_creates_valid_address_and_persists_on_node_and_is_broadcasted() {
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let amount_sat = 50_000;
        let network = cluster.networks.get(&node_peer).unwrap().clone();
        let (tx, mut rx) = unbounded_channel::<CreateDepositIntentResponse>();

        tokio::spawn(async move {
            let response = grpc_operator::create_deposit_intent(
                &network,
                CreateDepositIntentRequest {
                    public_key:
                        "020202020202020202020202020202020202020202020202020202020202020202"
                            .to_string(),
                    amount_satoshis: amount_sat,
                },
            )
            .await
            .expect("Failed to create deposit intent");
            tx.send(response).unwrap();
        });

        cluster.run_n_iterations(10).await;

        let response = rx.recv().await.unwrap();

        for (_, node) in cluster.nodes.iter_mut() {
            let intent_opt = match node
                .chain_interface_tx
                .send_message_with_response(abci::ChainMessage::GetDepositIntentByAddress {
                    address: response.deposit_address.clone(),
                })
                .await
            {
                Ok(abci::ChainResponse::GetDepositIntentByAddress { intent }) => intent,
                _ => None,
            };
            assert!(intent_opt.is_some(), "deposit intent not stored");
            let intent = intent_opt.unwrap();
            assert_eq!(intent.deposit_address, response.deposit_address);
        }
    }

    #[tokio::test]
    async fn create_deposit_state_generates_and_persists_intent() {
        // Arrange cluster with two peers to satisfy DKG assumptions (keys already seeded)
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        // Work with first node in cluster
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Create custom broadcast channel to observe deposit notifications
        let (tx, mut rx) = broadcast::channel::<DepositIntent>(4);

        // Instantiate fresh DepositIntentState using our tx instead of node default
        let mut state = DepositIntentState::new(tx.clone());

        let amount_sat = 42_000;

        // Act: invoke create_deposit
        let (_, deposit_address) = state
            .create_deposit(
                node,
                "020202020202020202020202020202020202020202020202020202020202020202",
                amount_sat,
            )
            .await
            .expect("create_deposit should succeed");

        // Assert: ABCI contains the new intent
        let stored = match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetDepositIntentByAddress {
                address: deposit_address.clone(),
            })
            .await
        {
            Ok(abci::ChainResponse::GetDepositIntentByAddress {
                intent: Some(intent),
            }) => intent,
            _ => panic!("intent not stored"),
        };
        assert_eq!(stored.deposit_address, deposit_address);
        assert_eq!(stored.amount_sat, amount_sat);
        assert_eq!(
            stored.user_pubkey,
            "020202020202020202020202020202020202020202020202020202020202020202"
        );

        // Assert: notification broadcast
        let notified_addr = rx.recv().await.expect("no broadcast received");
        assert_eq!(notified_addr.deposit_address, deposit_address);
    }

    #[tokio::test]
    async fn create_deposit_from_intent_persists_and_broadcasts() {
        // Setup cluster and node
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Broadcast channel for notifications
        let (tx, mut rx) = broadcast::channel::<DepositIntent>(4);
        let mut state = DepositIntentState::new(tx.clone());

        // Craft DepositIntent manually
        let deposit_tracking_id = Uuid::new_v4().to_string();
        let deposit_address = "tb1q62qxecgfyn7ud6esrxc50xh9hs56dysatwqheh".to_string();
        let deposit_intent = DepositIntent {
            amount_sat: 10_000,
            deposit_tracking_id: deposit_tracking_id.clone(),
            deposit_address: deposit_address.clone(),
            timestamp: 0,
            user_pubkey: "020202020202020202020202020202020202020202020202020202020202020202"
                .to_string(),
        };

        // Act
        state
            .create_deposit_from_intent(node, deposit_intent.clone())
            .await
            .expect("create_deposit_from_intent failed");

        // Assert ABCI persistence
        let stored = match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetDepositIntentByAddress {
                address: deposit_address.clone(),
            })
            .await
        {
            Ok(abci::ChainResponse::GetDepositIntentByAddress {
                intent: Some(intent),
            }) => intent,
            _ => panic!("intent not stored in abci"),
        };
        assert_eq!(stored.deposit_address, deposit_address);

        // Assert notification via channel
        let notified_addr = rx.recv().await.unwrap();
        assert_eq!(notified_addr.deposit_address, deposit_address);
    }

    #[tokio::test]
    async fn update_user_balance_increases_balance_after_confirmation() {
        // Setup cluster
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Broadcast channels for DepositIntentState constructor
        let (addr_tx, _addr_rx) = broadcast::channel::<DepositIntent>(4);
        let mut state = DepositIntentState::new(addr_tx);

        // ----- Prepare user address and account -----
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, user_pubkey) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let user_btc_pubkey = bitcoin::PublicKey::from_slice(&user_pubkey.serialize()).unwrap();
        let user_address = Address::p2pkh(user_btc_pubkey, bitcoin::Network::Testnet);

        // Insert user account with zero balance
        setup_test_account(
            node,
            &user_address.to_string(),
            Account::new(user_address.to_string(), 0),
        )
        .await
        .unwrap();

        // ----- Prepare deposit address affiliated with node pubkey -----
        let frost_pubkey_bytes = node
            .pubkey_package
            .as_ref()
            .unwrap()
            .verifying_key()
            .serialize()
            .unwrap();
        let node_pubkey = bitcoin::PublicKey::from_slice(&frost_pubkey_bytes).unwrap();
        let internal_key = node_pubkey.inner.x_only_public_key().0;
        let deposit_address = Address::p2tr(&secp, internal_key, None, bitcoin::Network::Testnet);

        state.deposit_addresses.insert(deposit_address.to_string());

        node.wallet.add_address(deposit_address.clone());

        let deposit_amount_sat = 15_000;

        match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::InsertDepositIntent {
                intent: DepositIntent {
                    amount_sat: deposit_amount_sat,
                    user_pubkey: user_address.to_string(), // must match account key
                    deposit_tracking_id: Uuid::new_v4().to_string(),
                    deposit_address: deposit_address.to_string(),
                    timestamp: 0,
                },
            })
            .await
        {
            Ok(abci::ChainResponse::InsertDepositIntent { error: None }) => {}
            _ => panic!("Failed to insert deposit intent"),
        }

        // ----- Craft transaction -----
        let tx_in = {
            bitcoin::TxIn {
                previous_output: bitcoin::OutPoint {
                    txid: bitcoin::Txid::from_slice(&[9u8; 32]).unwrap(),
                    vout: 0,
                },
                script_sig: user_address.script_pubkey(),
                sequence: bitcoin::Sequence::ZERO,
                witness: bitcoin::witness::Witness::new(),
            }
        };

        let tx_out = bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(deposit_amount_sat),
            script_pubkey: deposit_address.script_pubkey(),
        };

        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![tx_in],
            output: vec![tx_out],
        };

        // ----- Call update_user_balance -----
        let balance_before = match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetAccount {
                address: user_address.to_string(),
            })
            .await
        {
            Ok(abci::ChainResponse::GetAccount {
                account: Some(account),
            }) => account.balance,
            _ => panic!("Failed to get account balance"),
        };

        state
            .update_user_balance(node, &tx)
            .await
            .expect("balance update failed");

        // --- Assert wallet updated with the new UTXO ---
        let txid = tx.compute_txid();
        let utxo_found = node.wallet.utxos.iter().any(|u| {
            u.utxo.outpoint.txid == txid
                && u.utxo.outpoint.vout == 0
                && u.address == deposit_address
        });
        assert!(utxo_found, "wallet did not ingest expected UTXO");

        let balance_after = match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetAccount {
                address: user_address.to_string(),
            })
            .await
        {
            Ok(abci::ChainResponse::GetAccount {
                account: Some(account),
            }) => account.balance,
            _ => panic!("Failed to get account balance"),
        };

        assert_eq!(balance_after, balance_before + deposit_amount_sat);
    }

    #[tokio::test]
    async fn update_user_balance_does_not_increase_balance_for_non_deposit_transactions() {
        // Setup cluster
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Broadcast channels for DepositIntentState constructor
        let (addr_tx, _addr_rx) = broadcast::channel::<DepositIntent>(4);
        let mut state = DepositIntentState::new(addr_tx);

        // ----- Prepare user address and account -----
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, user_pubkey) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let user_btc_pubkey = bitcoin::PublicKey::from_slice(&user_pubkey.serialize()).unwrap();
        let user_address = Address::p2pkh(user_btc_pubkey, bitcoin::Network::Testnet);

        // Insert user account with zero balance
        setup_test_account(
            node,
            &user_address.to_string(),
            Account::new(user_address.to_string(), 0),
        )
        .await
        .unwrap();

        // ----- Craft transaction -----
        let deposit_amount_sat = 15_000;
        let tx_in = {
            bitcoin::TxIn {
                previous_output: bitcoin::OutPoint {
                    txid: bitcoin::Txid::from_slice(&[9u8; 32]).unwrap(),
                    vout: 0,
                },
                script_sig: user_address.script_pubkey(),
                sequence: bitcoin::Sequence::ZERO,
                witness: bitcoin::witness::Witness::new(),
            }
        };

        let tx_out = bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(deposit_amount_sat),
            script_pubkey: user_address.script_pubkey(),
        };

        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![tx_in],
            output: vec![tx_out],
        };

        // ----- Call update_user_balance -----
        state
            .update_user_balance(node, &tx)
            .await
            .expect("balance update failed");

        // Assert: balance should not have changed
        let balance = match node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetAccount {
                address: user_address.to_string(),
            })
            .await
        {
            Ok(abci::ChainResponse::GetAccount {
                account: Some(account),
            }) => account.balance,
            _ => 0, // If no account found, balance is 0
        };
        assert_eq!(balance, 0);

        // Wallet UTXOs should remain unchanged (none ingested)
        assert!(
            node.wallet.utxos.is_empty(),
            "wallet should not have ingested any UTXO"
        );
    }
}
