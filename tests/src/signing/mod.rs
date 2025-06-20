#[cfg(test)]
pub mod signing_tests {
    use std::str::FromStr;

    use crate::mocks::network::MockOracle;
    use bitcoin::{Address, Amount, Network, OutPoint, Txid, hashes::Hash};
    use node::wallet::{TaprootWallet, Wallet, taproot::TrackedUtxo};
    use types::utxo::Utxo;

    use crate::mocks::network::MockNodeCluster;
    use rand::RngCore;
    use types::network::network_event::{DirectMessage, NetworkEvent, SelfRequest};

    #[tokio::test]
    async fn signing_flow_completes_and_produces_shares() {
        let peers = 3;

        // Build cluster with pre-generated FROST keys – no DKG needed
        let mut cluster = MockNodeCluster::new_with_keys(peers).await;
        cluster.setup().await;

        // ── start signing ──
        let initiator = *cluster.nodes.keys().next().unwrap();
        let mut msg = [0u8; 32];
        rand::rng().fill_bytes(&mut msg);
        let hex_msg = hex::encode(msg);

        cluster.send_self_request_to_peer(
            initiator,
            SelfRequest::StartSigningSession {
                hex_message: hex_msg,
            },
        );

        // ── drive the network and count messages ──
        let (mut req, mut comm, mut pack, mut share) = (0, 0, 0, 0);
        for _ in 0..100 {
            cluster.run_n_iterations(1).await;

            // count DirectMessage traffic still queued in the mock senders
            for sender in cluster.senders.values() {
                for ev in &sender.pending_events {
                    if let NetworkEvent::MessageEvent((_, dm)) = ev {
                        match dm {
                            DirectMessage::SignRequest { .. } => req += 1,
                            DirectMessage::Commitments { .. } => comm += 1,
                            DirectMessage::SignPackage { .. } => pack += 1,
                            DirectMessage::SignatureShare { .. } => share += 1,
                            _ => {}
                        }
                    }
                }
            }
            // // break once no messages are left in either channel
            if cluster
                .senders
                .values()
                .all(|s| s.pending_events.is_empty())
            {
                break;
            }
        }

        // ── explicit assertions ──
        assert!(req >= peers as usize - 1, "missing SignRequest(s)");
        assert!(comm >= peers as usize - 1, "missing Commitments");
        assert!(pack >= peers as usize - 1, "missing SignPackage");
        assert!(share >= peers as usize - 1, "missing SignatureShare");

        // protocol should be finished – no pending spends
        for node in cluster.nodes.values() {
            let signing_state = node
                .handlers
                .iter()
                .find_map(|h| h.downcast_ref::<node::handlers::signing::SigningState>());
            assert!(
                signing_state.unwrap().active_signing.is_none(),
                "node still has pending signing {:?}",
                node.peer_id
            );
        }
    }

    fn create_test_wallet() -> TaprootWallet {
        let (events_emitter, _) = tokio::sync::broadcast::channel(100);
        let (deposits_emitter, _) = tokio::sync::broadcast::channel(100);
        let oracle = MockOracle::new(events_emitter, Some(deposits_emitter));
        TaprootWallet::new(Box::new(oracle), vec![], Network::Regtest)
    }

    fn create_dummy_utxo(
        value_sat: u64,
        address_str: &str,
        txid_byte: u8,
        vout: u32,
    ) -> TrackedUtxo {
        let address = Address::from_str(address_str).unwrap().assume_checked();
        let txid = Txid::from_byte_array([txid_byte; 32]);
        TrackedUtxo {
            utxo: Utxo {
                outpoint: OutPoint::new(txid, vout),
                value: Amount::from_sat(value_sat),
                script_pubkey: address.script_pubkey(),
            },
            address,
        }
    }

    #[test]
    fn test_change_consolidation_with_lowest_address_in_wallet() {
        let mut wallet = create_test_wallet();

        let addr_input = "tb1pm5y7ps8v24r9l9pvgu8p4dcusnueuayavc9xcx5ze2z7t485gdcq6dzg7z";
        let addr_low = "tb1pxpqezzaf7mk59tt5kgmpc4lvvjkx0zh3xhjre9cf9vspnlgrer3se036nk";

        wallet.utxos = vec![
            create_dummy_utxo(70_000, addr_input, 1, 0),
            create_dummy_utxo(10_000, addr_low, 2, 0),
        ];

        let recipient_addr =
            Address::from_str("tb1pwvn7aqsgrh7msgmpj9knrenglvdmsqljsads363696k2368x2kwsd5wztk")
                .unwrap()
                .assume_checked();

        let (tx, _) = wallet
            .create_spend(60_000, 1_000, &recipient_addr, false)
            .unwrap();

        assert_eq!(tx.input.len(), 1);
        assert_eq!(tx.output.len(), 2);

        let change_output = tx
            .output
            .iter()
            .find(|o| o.value == Amount::from_sat(9_000))
            .unwrap();

        let expected_change_address = Address::from_str(addr_low).unwrap().assume_checked();
        assert_eq!(
            change_output.script_pubkey,
            expected_change_address.script_pubkey()
        );

        let final_balance_at_lowest_addr: u64 = wallet
            .get_utxos()
            .iter()
            .filter(|u| u.address == expected_change_address)
            .map(|u| u.utxo.value.to_sat())
            .sum();

        assert_eq!(final_balance_at_lowest_addr, 19_000);
    }

    #[test]
    fn test_spend_with_no_change() {
        let mut wallet = create_test_wallet();

        wallet.utxos = vec![create_dummy_utxo(
            50_000,
            "tb1pm5y7ps8v24r9l9pvgu8p4dcusnueuayavc9xcx5ze2z7t485gdcq6dzg7z",
            1,
            0,
        )];

        let recipient_addr =
            Address::from_str("tb1pwvn7aqsgrh7msgmpj9knrenglvdmsqljsads363696k2368x2kwsd5wztk")
                .unwrap()
                .assume_checked();

        let (tx, _) = wallet
            .create_spend(49_000, 1_000, &recipient_addr, false)
            .unwrap();

        assert_eq!(tx.input.len(), 1);
        assert_eq!(tx.output.len(), 1);
        assert_eq!(tx.output[0].value, Amount::from_sat(49_000));
    }

    #[test]
    fn test_spend_from_single_utxo_with_change() {
        let mut wallet = create_test_wallet();
        let utxo_addr_str = "tb1pm5y7ps8v24r9l9pvgu8p4dcusnueuayavc9xcx5ze2z7t485gdcq6dzg7z";
        let utxo_addr = Address::from_str(utxo_addr_str).unwrap().assume_checked();

        wallet.utxos = vec![create_dummy_utxo(50_000, utxo_addr_str, 1, 0)];

        let recipient_addr =
            Address::from_str("tb1pwvn7aqsgrh7msgmpj9knrenglvdmsqljsads363696k2368x2kwsd5wztk")
                .unwrap()
                .assume_checked();

        let (tx, _) = wallet
            .create_spend(30_000, 1_000, &recipient_addr, false)
            .unwrap();

        assert_eq!(tx.input.len(), 1);
        assert_eq!(tx.output.len(), 2);

        let change_output = tx
            .output
            .iter()
            .find(|o| o.value == Amount::from_sat(19_000))
            .unwrap();

        assert_eq!(change_output.script_pubkey, utxo_addr.script_pubkey());

        let final_balance: u64 = wallet
            .get_utxos()
            .iter()
            .map(|u| u.utxo.value.to_sat())
            .sum();

        assert_eq!(final_balance, 19_000);
    }
}
