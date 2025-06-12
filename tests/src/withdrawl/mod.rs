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
            &hex::encode(public_key.serialize()),
            Account {
                address: hex::encode(public_key.serialize()),
                balance: 100_000,
            },
        );

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
            &hex::encode(public_key.serialize()),
            Account {
                address: hex::encode(public_key.serialize()),
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

    #[tokio::test]
    async fn confirm_withdrawal_generates_tx_and_updates_peers() {
        let mut cluster = MockNodeCluster::new_with_keys(3, 2, 3).await;
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

        // Populate chain state on **all** peers before we take any mutable references to a single node
        for (_peer, node) in cluster.nodes.iter_mut() {
            node.chain_state.upsert_account(
                &public_key_hex,
                Account {
                    address: public_key_hex.clone(),
                    balance: initial_balance,
                },
            );

            node.wallet.address = Some(dest_addr.clone());
            node.wallet.utxos.push(Utxo {
                outpoint: OutPoint {
                    txid: Txid::from_slice(&[3u8; 32]).unwrap(),
                    vout: 0,
                },
                value: Amount::from_sat(initial_balance),
                script_pubkey: dest_addr.script_pubkey(),
            });
        }

        // --- Step 1: Propose the withdrawal ---
        let amount_sat = 50_000u64;

        // Spawn the propose_withdrawal request so the cluster can process events concurrently
        let (prop_tx, mut prop_rx) = unbounded_channel::<ProposeWithdrawalResponse>();
        let network_clone = initiator_network.clone();
        let dest_addr_str = dest_addr.to_string();
        let pubkey_hex_clone = public_key_hex.clone();
        tokio::spawn(async move {
            let resp = node::grpc::grpc_operator::propose_withdrawal(
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
            let _ = node::grpc::grpc_operator::confirm_withdrawal(
                &network_clone2,
                node::grpc::grpc_handler::node_proto::ConfirmWithdrawalRequest {
                    challenge: challenge_hex.clone(),
                    signature: signature_hex,
                },
            )
            .await
            .expect("Failed to confirm withdrawal");
        });

        // Run enough iterations so that signing workflow and gossipsub propagation complete
        cluster.run_n_iterations(10).await;

        // --- Assert: every peer has updated the user's balance ---
        for (_, node) in cluster.nodes.iter() {
            let account = node
                .chain_state
                .get_account(&public_key_hex)
                .expect("Account should exist on all peers");
            assert_eq!(account.balance, initial_balance - amount_sat);
        }
    }
}
