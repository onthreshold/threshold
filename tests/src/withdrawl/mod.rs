#[cfg(test)]
mod withdrawl_tests {
    use bitcoin::{Address, Amount, CompressedPublicKey, OutPoint, Txid, hashes::Hash};
    use node::grpc::{
        grpc_handler::node_proto::{ProposeWithdrawalRequest, ProposeWithdrawalResponse},
        grpc_operator,
    };

    use crate::mocks::network::MockNodeCluster;
    use node::handlers::withdrawl::{SpendIntent, SpendIntentState};
    use protocol::{chain_state::Account, oracle::Utxo};
    use std::collections::HashMap;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn propose_withdrawal_returns_quote_and_challenge() {
        // Arrange: create a mock cluster
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        // Select a single node & associated network handle
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let network = cluster.networks.get(&node_peer).unwrap().clone();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // --- prepare wallet & chain state so that the withdrawal can succeed ---
        // Generate a dummy address on Signet network for both the wallet and withdrawal target
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, public_key) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let btc_pubkey = CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        let address = Address::p2wpkh(&btc_pubkey, bitcoin::Network::Signet);

        // Provide the node with sufficient on-chain balance for this address
        node.chain_state.upsert_account(
            &address.to_string(),
            Account {
                address: address.to_string(),
                balance: 100_000,
            },
        );

        // Add a dummy UTXO so that wallet::create_spend succeeds
        node.wallet.address = Some(address.clone());
        node.wallet.utxos.push(Utxo {
            outpoint: OutPoint {
                txid: Txid::from_slice(&[1u8; 32]).unwrap(),
                vout: 0,
            },
            value: Amount::from_sat(100_000),
            script_pubkey: address.script_pubkey(),
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
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Create a dummy address but DO NOT fund it sufficiently
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, public_key) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let btc_pubkey = bitcoin::PublicKey::from_slice(&public_key.serialize()).unwrap();
        let address = Address::p2pkh(btc_pubkey, bitcoin::Network::Signet);

        // Insert account with low balance (e.g., 1 satoshi)
        node.chain_state.upsert_account(
            &address.to_string(),
            Account {
                address: address.to_string(),
                balance: 1,
            },
        );

        // Prepare SpendIntent and state
        let mut spend_state = SpendIntentState {
            pending_intents: HashMap::new(),
        };

        let withdrawal_intent = SpendIntent {
            amount_sat: 50_000,
            address_to: address.to_string(),
            public_key: hex::encode(public_key.serialize()),
            blocks_to_confirm: None,
        };

        // Act
        let result = spend_state
            .propose_withdrawal(node, &withdrawal_intent)
            .await;

        // Assert: should error due to insufficient balance
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn confirm_withdrawal_fails_invalid_signature() {
        // Setup cluster
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        // Extract node
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Prepare funding and accounts
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let (_, public_key) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
        let btc_pubkey = CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        let address = Address::p2wpkh(&btc_pubkey, bitcoin::Network::Signet);

        // Fund account and wallet UTXO so propose succeeds
        node.chain_state.upsert_account(
            &address.to_string(),
            Account {
                address: address.to_string(),
                balance: 100_000,
            },
        );

        node.wallet.address = Some(address.clone());
        node.wallet.utxos.push(Utxo {
            outpoint: OutPoint {
                txid: Txid::from_slice(&[2u8; 32]).unwrap(),
                vout: 0,
            },
            value: Amount::from_sat(100_000),
            script_pubkey: address.script_pubkey(),
        });

        // SpendIntentState under test
        let mut spend_state = SpendIntentState {
            pending_intents: HashMap::new(),
        };

        let withdrawal_intent = SpendIntent {
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
        let result = spend_state
            .confirm_withdrawal(node, &challenge, "deadbeef")
            .await;

        // Expect error
        assert!(result.is_err());

        // And the intent should have been removed from pending_intents
        assert!(!spend_state.pending_intents.contains_key(&challenge));
    }
}
