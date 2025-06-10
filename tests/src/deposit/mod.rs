#[cfg(test)]
mod deposit_tests {
    use std::str::FromStr;

    use crate::mocks::network::MockNodeCluster;
    use bitcoin::Address;
    use node::{
        db::Db,
        grpc::{
            grpc_handler::node_proto::{CreateDepositIntentRequest, CreateDepositIntentResponse},
            grpc_operator,
        },
    };
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn deposit_intent_creates_valid_address_and_persists_on_node() {
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let user_id = node_peer.to_string();
        let amount_sat = 50_000;
        let (tx, mut rx) = unbounded_channel::<CreateDepositIntentResponse>();
        let network = cluster.networks.get(&node_peer).unwrap().clone();

        tokio::spawn(async move {
            let response = grpc_operator::create_deposit_intent(
                &network,
                CreateDepositIntentRequest {
                    user_id: user_id.clone(),
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
        assert!(addr.is_valid_for_network(bitcoin::Network::Signet));
    }

    #[tokio::test]
    async fn deposit_intent_creates_valid_address_and_persists_on_node_and_is_broadcasted() {
        let mut cluster = MockNodeCluster::new_with_keys(2, 2, 2).await;
        cluster.setup().await;

        let node_peer = *cluster.nodes.keys().next().unwrap();
        let user_id = node_peer.to_string();
        let amount_sat = 50_000;
        let network = cluster.networks.get(&node_peer).unwrap().clone();
        let (tx, mut rx) = unbounded_channel::<CreateDepositIntentResponse>();

        tokio::spawn(async move {
            let response = grpc_operator::create_deposit_intent(
                &network,
                CreateDepositIntentRequest {
                    user_id: user_id.clone(),
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
}
