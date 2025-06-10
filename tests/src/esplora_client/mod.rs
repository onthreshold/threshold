#[cfg(test)]
mod esplora_client_test {
    use bitcoin::{Address, Network};
    use clients::{EsploraApiClient, WindowedConfirmedTransactionProvider};
    use std::str::FromStr;

    #[tokio::test]
    async fn test_get_confirmed_transactions() {
        let client = EsploraApiClient::new_with_network(Network::Bitcoin, Some(100), None);
        let address = Address::from_str("bc1qezwz3yt46nsgzcwlg0dsw680nryjpq5u8pvzts")
            .unwrap()
            .require_network(Network::Bitcoin)
            .unwrap();
        let transactions = client
            .get_confirmed_transactions(vec![address.clone()], 899900, 899930)
            .await
            .unwrap();

        let correct_txs = [
            "99c024e891c3110297513a1bc8c6f36948b36461096e664be72c3ac96e958c5c",
            "1d0249929acaf31c2c6b6e6f9c72f44bd663a426cb146afe0b7bbaa66e0bc0df",
            "fdcd9cf8d660e359a6ab2993d649276fca60be01c2b4327f95ad2527cbe3db08",
            "3fd280c3ccc13f0f88433f0ce95aeebacc249565c8e8b671005302de0616babe",
            "a8705186a9d6b5063484a8029b0e2c4064e3e2723ea61ea10b6bc38d0abbc77a",
        ];

        assert_eq!(transactions.len(), correct_txs.len());

        for tx in transactions {
            assert!(correct_txs.contains(&tx.compute_txid().to_string().as_str()));
        }
    }
}
