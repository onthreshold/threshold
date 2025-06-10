#[cfg(test)]
mod deposit_tests {
    use std::str::FromStr;

    use crate::mocks::network::MockNodeCluster;
    use bitcoin::Address;
    use node::{
        db::Db,
        deposit::{DepositIntent, DepositIntentState},
        grpc::{
            grpc_handler::node_proto::{CreateDepositIntentRequest, CreateDepositIntentResponse},
            grpc_operator,
        },
    };
    use tokio::sync::broadcast;
    use tokio::sync::mpsc::unbounded_channel;
    use uuid::Uuid;

    #[tokio::test]
    async fn deposit_intent_creates_valid_address_and_persists_on_node() {
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let amount_sat = 50_000;
        let (tx, mut rx) = unbounded_channel::<CreateDepositIntentResponse>();
        let network = cluster.networks.get(&node_peer).unwrap().clone();

        tokio::spawn(async move {
            let response = grpc_operator::create_deposit_intent(
                &network,
                CreateDepositIntentRequest {
                    amount_satoshis: amount_sat,
                },
            )
            .await
            .expect("Failed to create deposit intent");
            tx.send(response).unwrap();
        });

        cluster.run_n_iterations(10).await;

        let response = rx.recv().await.unwrap();
        let node = cluster.nodes.get(&node_peer).unwrap();
        let db = &node.db;

        // retrieve the first deposit intent stored
        let intent_opt = db
            .get_deposit_intent(&response.deposit_tracking_id)
            .unwrap();

        assert!(intent_opt.is_some(), "deposit intent not stored");
        let intent = intent_opt.unwrap();

        // parse address and validate
        let addr = Address::from_str(&intent.deposit_address).unwrap();

        let is_testnet: bool = std::env::var("IS_TESTNET")
            .unwrap_or("false".to_string())
            .parse()
            .unwrap();
        assert!(addr.is_valid_for_network(if is_testnet {
            bitcoin::Network::Testnet
        } else {
            bitcoin::Network::Bitcoin
        }));
    }

    #[tokio::test]
    async fn deposit_intent_creates_valid_address_and_persists_on_node_and_is_broadcasted() {
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let amount_sat = 50_000;
        let network = cluster.networks.get(&node_peer).unwrap().clone();
        let (tx, mut rx) = unbounded_channel::<CreateDepositIntentResponse>();

        tokio::spawn(async move {
            let response = grpc_operator::create_deposit_intent(
                &network,
                CreateDepositIntentRequest {
                    amount_satoshis: amount_sat,
                },
            )
            .await
            .expect("Failed to create deposit intent");
            tx.send(response).unwrap();
        });

        cluster.run_n_iterations(10).await;

        let response = rx.recv().await.unwrap();

        for (_, node) in cluster.nodes.iter() {
            let intent_opt = node
                .db
                .get_deposit_intent(&response.deposit_tracking_id)
                .unwrap();
            assert!(intent_opt.is_some(), "deposit intent not stored");
            let intent = intent_opt.unwrap();
            assert_eq!(intent.deposit_address, response.deposit_address);
        }
    }

    #[tokio::test]
    async fn create_deposit_state_generates_and_persists_intent() {
        // Arrange cluster with two peers to satisfy DKG assumptions (keys already seeded)
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        // Work with first node in cluster
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Create custom broadcast channel to observe deposit notifications
        let (tx, mut rx) = broadcast::channel::<String>(4);

        // Instantiate fresh DepositIntentState using our tx instead of node default
        let mut state = DepositIntentState::new(tx.clone());

        let amount_sat = 42_000;

        // Act: invoke create_deposit
        let (tracking_id, deposit_address) = state
            .create_deposit(node, amount_sat)
            .await
            .expect("create_deposit should succeed");

        // Assert: DB contains the new intent
        let stored = node
            .db
            .get_deposit_intent(&tracking_id)
            .expect("db query failed")
            .expect("intent not stored");
        assert_eq!(stored.deposit_address, deposit_address);
        assert_eq!(stored.amount_sat, amount_sat);

        // Assert: notification broadcast
        let notified_addr = rx.recv().await.expect("no broadcast received");
        assert_eq!(notified_addr, deposit_address);
    }

    #[tokio::test]
    async fn create_deposit_from_intent_persists_and_broadcasts() {
        // Setup cluster and node
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;
        let node_peer = *cluster.nodes.keys().next().unwrap();
        let node = cluster.nodes.get_mut(&node_peer).unwrap();

        // Broadcast channel for notifications
        let (tx, mut rx) = broadcast::channel::<String>(4);
        let mut state = DepositIntentState::new(tx.clone());

        // Craft DepositIntent manually
        let deposit_tracking_id = Uuid::new_v4().to_string();
        let deposit_address = "tb1qexampleaddressxxxx0000".to_string();
        let deposit_intent = DepositIntent {
            amount_sat: 10_000,
            deposit_tracking_id: deposit_tracking_id.clone(),
            deposit_address: deposit_address.clone(),
            timestamp: 0,
        };

        // Act
        state
            .create_deposit_from_intent(node, deposit_intent.clone())
            .expect("create_deposit_from_intent failed");

        // Assert DB persistence
        let stored = node
            .db
            .get_deposit_intent(&deposit_tracking_id)
            .unwrap()
            .expect("intent not stored in db");
        assert_eq!(stored.deposit_address, deposit_address);

        // Assert notification via channel
        let notified_addr = rx.recv().await.unwrap();
        assert_eq!(notified_addr, deposit_address);
    }
}
