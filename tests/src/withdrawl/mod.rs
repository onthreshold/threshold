#[cfg(test)]
mod withdrawl_tests {
    async fn setup_account_with_balance(
        node: &mut crate::mocks::network::MockNodeState,
        public_key_hex: &str,
        balance: u64,
    ) {
        let account_transaction = protocol::transaction::Transaction::new(
            protocol::transaction::TransactionType::Deposit,
            vec![
                protocol::transaction::Operation::OpPush {
                    value: balance.to_be_bytes().to_vec(),
                },
                protocol::transaction::Operation::OpPush {
                    value: public_key_hex.as_bytes().to_vec(),
                },
                protocol::transaction::Operation::OpPush {
                    value: format!("mock_txid_for_withdrawal_test_{}", public_key_hex)
                        .as_bytes()
                        .to_vec(),
                },
                protocol::transaction::Operation::OpCheckOracle,
                protocol::transaction::Operation::OpPush {
                    value: balance.to_be_bytes().to_vec(),
                },
                protocol::transaction::Operation::OpPush {
                    value: public_key_hex.as_bytes().to_vec(),
                },
                protocol::transaction::Operation::OpIncrementBalance,
            ],
            None,
        );

        node.chain_interface_tx
            .send_message_with_response(abci::ChainMessage::AddTransactionToBlock {
                transaction: account_transaction,
            })
            .await
            .expect("Failed to add account setup transaction");

        let setup_block = node
            .chain_interface_tx
            .send_message_with_response(abci::ChainMessage::GetProposedBlock {
                previous_block: None,
                proposer: vec![1, 2, 3, 4],
            })
            .await
            .expect("Failed to get proposed block");

        if let abci::ChainResponse::GetProposedBlock { block } = setup_block {
            node.chain_interface_tx
                .send_message_with_response(abci::ChainMessage::FinalizeBlock { block })
                .await
                .expect("Failed to finalize setup block");
        }
    }
    use bitcoin::{Address, Amount, CompressedPublicKey, OutPoint, Txid, hashes::Hash};
    use node::wallet::TrackedUtxo;
    use types::proto::node_proto::{
        ConfirmWithdrawalRequest, ProposeWithdrawalRequest, ProposeWithdrawalResponse,
    };

    use crate::mocks::network::MockNodeCluster;
    use grpc::grpc_operator;
    use node::handlers::withdrawl::SpendIntentState;
    use std::collections::HashMap;
    use tokio::sync::mpsc::unbounded_channel;
    use types::intents::WithdrawlIntent;
    use types::utxo::Utxo;

    #[tokio::test]
    async fn propose_withdrawal_returns_quote_and_challenge() {
        // Arrange: create a mock cluster
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        // Select a single node & associated network handle
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let network = cluster.networks.get(&node_peer).unwrap().clone();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, public_key) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let btc_pubkey = CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        let address = Address::p2wpkh(&btc_pubkey, bitcoin::Network::Signet);

        setup_account_with_balance(node, &hex::encode(public_key.serialize()), 100_000).await;

        let utxo = Utxo {
            outpoint: OutPoint {
                txid: Txid::from_slice(&[2u8; 32]).unwrap(),
                vout: 0,
            },
            value: Amount::from_sat(100_000),
            script_pubkey: address.script_pubkey(),
        };

        node.wallet.utxos.push(TrackedUtxo {
            utxo,
            address: address.clone(),
        });

        // Build the withdrawal request
        let public_key_hex = hex::encode(public_key.serialize());
        let amount_sat = 50_000;

        let (tx, mut rx) = unbounded_channel::<ProposeWithdrawalResponse>();
        let network_clone = network.clone();
        let address_str = address.to_string();
        tokio::spawn(async move {
            let response = grpc_operator::propose_withdrawal(
                &network_clone,
                ProposeWithdrawalRequest {
                    amount_satoshis: amount_sat,
                    address_to: address_str,
                    public_key: public_key_hex,
                    blocks_to_confirm: None,
                },
            )
            .await
            .expect("Failed to propose withdrawal");
            tx.send(response).unwrap();
        });

        // Act: run the cluster event loop a few iterations so the request is processed
        cluster.run_n_iterations(10).await;

        // Assert: we received a response with sensible values
        let response = rx.recv().await.unwrap();
        assert!(response.quote_satoshis > amount_sat);
        assert_eq!(response.challenge.len(), 64);
    }

    #[tokio::test]
    async fn propose_withdrawal_insufficient_balance() {
        // Setup minimal cluster and node
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Create a dummy address but DO NOT fund it sufficiently
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, public_key) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let btc_pubkey = bitcoin::PublicKey::from_slice(&public_key.serialize()).unwrap();
        let address = Address::p2pkh(btc_pubkey, bitcoin::Network::Signet);

        // Prepare SpendIntent and state
        let mut spend_state = SpendIntentState {
            pending_intents: HashMap::new(),
        };

        let withdrawal_intent = WithdrawlIntent {
            amount_sat: 50_000,
            address_to: address.to_string(),
            public_key: hex::encode(public_key.serialize()),
            blocks_to_confirm: None,
        };

        let result = spend_state
            .propose_withdrawal(node, &withdrawal_intent)
            .await;

        assert!(
            result.is_err(),
            "Expected withdrawal to fail without sufficient setup"
        );
    }

    #[tokio::test]
    async fn confirm_withdrawal_fails_invalid_signature() {
        // Setup cluster
        let mut cluster = MockNodeCluster::new_with_keys(2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, public_key) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let btc_pubkey = CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        let address = Address::p2wpkh(&btc_pubkey, bitcoin::Network::Signet);

        setup_account_with_balance(node, &hex::encode(public_key.serialize()), 100_000).await;

        let utxo = Utxo {
            outpoint: OutPoint {
                txid: Txid::from_slice(&[2u8; 32]).unwrap(),
                vout: 0,
            },
            value: Amount::from_sat(100_000),
            script_pubkey: address.script_pubkey(),
        };

        node.wallet.utxos.push(TrackedUtxo {
            utxo,
            address: address.clone(),
        });

        // SpendIntentState under test
        let mut spend_state = SpendIntentState {
            pending_intents: HashMap::new(),
        };

        let withdrawal_intent = WithdrawlIntent {
            amount_sat: 50_000,
            address_to: address.to_string(),
            public_key: hex::encode(public_key.serialize()),
            blocks_to_confirm: None,
        };

        // First propose to obtain challenge
        let (_, challenge) = spend_state
            .propose_withdrawal(node, &withdrawal_intent)
            .await
            .expect("Propose withdrawal should succeed");

        // Now attempt to confirm with an obviously invalid signature
        let result = spend_state.confirm_withdrawal(node, &challenge, "deadbeef");

        // Expect error
        assert!(result.is_err());

        // And the intent should have been removed from pending_intents
        assert!(!spend_state.pending_intents.contains_key(&challenge));
    }

    #[tokio::test]
    async fn confirm_withdrawal_generates_tx_and_updates_peers() {
        let mut cluster = MockNodeCluster::new_with_keys(3).await;
        cluster.setup().await;

        // Select an initiating node and its network handle
        let initiator_peer = *cluster.nodes.keys().next().unwrap();
        let initiator_network = cluster.networks.get(&initiator_peer).unwrap().clone();

        // Generate a fresh keypair for the user
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (secret_key, public_key) =
            secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let public_key_hex = hex::encode(public_key.serialize());

        // Destination (withdrawal) address derived from the same pubkey for simplicity
        let btc_pubkey = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        let dest_addr = bitcoin::Address::p2wpkh(&btc_pubkey, bitcoin::Network::Signet);

        // Initial balance for user & destination address
        let initial_balance = 100_000u64;

        for (_peer, node) in cluster.nodes.iter_mut() {
            setup_account_with_balance(node, &public_key_hex, initial_balance).await;

            let utxo = Utxo {
                outpoint: OutPoint {
                    txid: Txid::from_slice(&[3u8; 32]).unwrap(),
                    vout: 0,
                },
                value: Amount::from_sat(initial_balance),
                script_pubkey: dest_addr.script_pubkey(),
            };

            node.wallet.utxos.push(TrackedUtxo {
                utxo,
                address: dest_addr.clone(),
            });
        }

        // Keep a reference to the original outpoint that should be spent
        let original_outpoint = bitcoin::OutPoint {
            txid: Txid::from_slice(&[3u8; 32]).unwrap(),
            vout: 0,
        };

        // --- Step 1: Propose the withdrawal ---
        let amount_sat = 50_000u64;

        // Spawn the propose_withdrawal request so the cluster can process events concurrently
        let (prop_tx, mut prop_rx) = unbounded_channel::<ProposeWithdrawalResponse>();
        let network_clone = initiator_network.clone();
        let dest_addr_str = dest_addr.to_string();
        let pubkey_hex_clone = public_key_hex.clone();
        tokio::spawn(async move {
            let resp = grpc_operator::propose_withdrawal(
                &network_clone,
                ProposeWithdrawalRequest {
                    amount_satoshis: amount_sat,
                    address_to: dest_addr_str,
                    public_key: pubkey_hex_clone,
                    blocks_to_confirm: None,
                },
            )
            .await
            .expect("Failed to propose withdrawal");
            prop_tx.send(resp).unwrap();
        });

        // Run a few iterations so that the proposal is processed and we receive the challenge
        cluster.run_n_iterations(10).await;

        let propose_resp = prop_rx.recv().await.expect("No propose response");

        // --- Step 2: Sign the received challenge ---
        let challenge_hex = propose_resp.challenge;
        let challenge_bytes = hex::decode(&challenge_hex).unwrap();
        let msg = bitcoin::secp256k1::Message::from_digest_slice(&challenge_bytes).unwrap();
        let signature = secp.sign_ecdsa(&msg, &secret_key);
        let signature_hex = hex::encode(signature.serialize_der());

        // --- Step 3: Confirm the withdrawal with the valid signature ---
        let network_clone2 = initiator_network.clone();
        tokio::spawn(async move {
            let _ = grpc_operator::confirm_withdrawal(
                &network_clone2,
                ConfirmWithdrawalRequest {
                    challenge: challenge_hex.clone(),
                    signature: signature_hex,
                },
            )
            .await
            .expect("Failed to confirm withdrawal");
        });

        // Run enough iterations so that signing workflow and gossipsub propagation complete
        cluster.run_n_iterations(10).await;

        for (_, node) in cluster.nodes.iter_mut() {
            let pending_transactions = match node
                .chain_interface_tx
                .send_message_with_response(abci::ChainMessage::GetPendingTransactions)
                .await
            {
                Ok(abci::ChainResponse::GetPendingTransactions { transactions }) => transactions,
                _ => panic!("Failed to get pending transactions"),
            };

            assert!(
                !pending_transactions.is_empty(),
                "Expected withdrawal transaction to be pending"
            );

            let withdrawal_transaction = pending_transactions
                .iter()
                .find(|tx| tx.r#type == protocol::transaction::TransactionType::Withdrawal)
                .expect("Expected to find withdrawal transaction");

            let operations = &withdrawal_transaction.operations;
            assert!(
                operations.len() >= 3,
                "Expected at least 3 operations for withdrawal transaction"
            );

            if let protocol::transaction::Operation::OpPush { value } = &operations[1] {
                let address_from_tx =
                    String::from_utf8(value.clone()).expect("Invalid address in transaction");
                assert_eq!(address_from_tx, public_key_hex);
            } else {
                panic!("Expected OpPush with address as second operation");
            }

            let spent_still_present = node
                .wallet
                .utxos
                .iter()
                .any(|u| u.utxo.outpoint == original_outpoint);
            assert!(!spent_still_present, "Spent UTXO still present in wallet");
        }
    }
}
