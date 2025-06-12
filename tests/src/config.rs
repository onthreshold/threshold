#[cfg(test)]
mod config_test {
    use node::NodeConfig;

    #[test]
    fn test_config_deserialization() {
        let json_str = r#"{
            "allowed_peers": [
                {
                    "public_key": "12D3KooWRdtE2nFybk8eMyp3D9B4NvunUYqpN6JDvBcVPTcrDsbF",
                    "name": "node-four"
                }
            ],
            "key_data": {
                "public_key_b58": "12D3KooWQDHzW448RmDoUz1KbMfuD4XqeojRJDsxqUZSEYo7FSUz",
                "encrypted_private_key_b64": "EnCF8bEe3tVyMV0EUIK29bOMNjH7gT7mx4ATyBr4WSdphw5ETfm1YdQHDAg+CzBBjt7K2FSbwv8Qkj1y3N4jTU/FkGHggfkwDDl5XkDc5rXi2BW/",
                "encryption_params": {
                    "kdf": "argon2id",
                    "salt_b64": "TnErEFlx9F1BeU8mJcFzKQ",
                    "iv_b64": "hybTge0qoPaxNUhP"
                }
            },
            "database_directory": "nodedb.db",
            "grpc_port": 50051,
            "libp2p_udp_port": 0,
            "libp2p_tcp_port": 0,
            "confirmation_depth": 6,
            "monitor_start_block": -1
        }"#;

        let config: NodeConfig = serde_json::from_str(json_str).expect("Failed to deserialize");
        assert_eq!(config.allowed_peers.len(), 1);
        assert_eq!(
            config.key_data.public_key_b58,
            "12D3KooWQDHzW448RmDoUz1KbMfuD4XqeojRJDsxqUZSEYo7FSUz"
        );
        assert!(config.dkg_keys.is_none());
        assert_eq!(
            config.database_directory,
            std::path::PathBuf::from("nodedb.db")
        );
        assert_eq!(config.grpc_port, 50051);
        assert_eq!(config.libp2p_udp_port, 0);
        assert_eq!(config.libp2p_tcp_port, 0);
        assert_eq!(config.confirmation_depth, 6);
        assert_eq!(config.monitor_start_block, -1);
    }
}
