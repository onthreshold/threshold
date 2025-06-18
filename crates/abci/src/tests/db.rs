use crate::chain_state::{Account, ChainState};
use crate::db::Db;
use crate::db::rocksdb::RocksDb;
use protocol::block::{Block, BlockBody, BlockHeader, ChainConfig, GenesisBlock, ValidatorInfo};
use std::collections::HashMap;
use tempfile::TempDir;
use types::intents::DepositIntent;
use uuid::Uuid;

fn create_test_db() -> (RocksDb, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().to_str().unwrap();
    let db = RocksDb::new(db_path);
    (db, temp_dir)
}

#[test]
fn test_rocksdb_new() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().to_str().unwrap();
    let db = RocksDb::new(db_path);
    assert!(db.db.live_files().is_ok());
}

#[test]
fn test_get_tip_block_hash_empty() {
    let (db, _temp_dir) = create_test_db();

    let result = db.get_tip_block_hash();
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_get_block_by_height_empty() {
    let (db, _temp_dir) = create_test_db();

    let result = db.get_block_by_height(0);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_get_block_by_hash_empty() {
    let (db, _temp_dir) = create_test_db();

    let hash = [0u8; 32];
    let result = db.get_block_by_hash(hash);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_get_chain_state_empty() {
    let (db, _temp_dir) = create_test_db();

    let result = db.get_chain_state();
    assert!(result.is_ok());

    let state = result.unwrap().unwrap();
    assert_eq!(state.get_block_height(), 0);
    assert_eq!(state.get_all_deposit_intents().len(), 0);
}

#[test]
fn test_insert_and_get_chain_state() {
    let (db, _temp_dir) = create_test_db();

    let mut chain_state = ChainState::new();
    chain_state.upsert_account("test_addr", Account::new("test_addr".to_string(), 1000));

    let insert_result = db.insert_chain_state(chain_state.clone());
    assert!(insert_result.is_ok());

    let get_result = db.get_chain_state();
    assert!(get_result.is_ok());

    let retrieved_state = get_result.unwrap().unwrap();
    let account = retrieved_state.get_account("test_addr").unwrap();
    assert_eq!(account.balance, 1000);
}

#[test]
fn test_flush_state() {
    let (db, _temp_dir) = create_test_db();

    let mut chain_state = ChainState::new();
    chain_state.upsert_account("flush_addr", Account::new("flush_addr".to_string(), 500));

    let flush_result = db.flush_state(&chain_state);
    assert!(flush_result.is_ok());

    let get_result = db.get_chain_state();
    assert!(get_result.is_ok());

    let retrieved_state = get_result.unwrap().unwrap();
    let account = retrieved_state.get_account("flush_addr").unwrap();
    assert_eq!(account.balance, 500);
}

#[test]
fn test_insert_and_get_block() {
    let (db, _temp_dir) = create_test_db();

    // Create a test genesis block
    let validators = vec![ValidatorInfo {
        pub_key: b"test_validator".to_vec(),
        stake: 1000,
    }];

    let chain_config = ChainConfig {
        min_signers: 1,
        max_signers: 1,
        min_stake: 1000,
        block_time_seconds: 10,
        max_block_size: 1_024_000,
    };

    let genesis_block = GenesisBlock::new(
        validators,
        chain_config,
        vec![1, 2, 3, 4], // Mock pubkey
    );

    let block = genesis_block.to_block();
    let block_hash = block.hash();

    // Insert the block
    let insert_result = db.insert_block(block);
    assert!(insert_result.is_ok());

    // Test get by height
    let get_by_height = db.get_block_by_height(0);
    assert!(get_by_height.is_ok());
    let retrieved_block = get_by_height.unwrap().unwrap();
    assert_eq!(retrieved_block.header.height, 0);
    assert_eq!(retrieved_block.hash(), block_hash);

    // Test get by hash
    let get_by_hash = db.get_block_by_hash(block_hash);
    assert!(get_by_hash.is_ok());
    let retrieved_block = get_by_hash.unwrap().unwrap();
    assert_eq!(retrieved_block.header.height, 0);

    // Test tip block hash
    let tip_hash = db.get_tip_block_hash();
    assert!(tip_hash.is_ok());
    assert_eq!(tip_hash.unwrap().unwrap(), block_hash);
}

#[test]
fn test_insert_multiple_blocks() {
    let (db, _temp_dir) = create_test_db();

    // Create and insert genesis block
    let validators = vec![ValidatorInfo {
        pub_key: b"test_validator".to_vec(),
        stake: 1000,
    }];

    let chain_config = ChainConfig {
        min_signers: 1,
        max_signers: 1,
        min_stake: 1000,
        block_time_seconds: 10,
        max_block_size: 1_024_000,
    };

    let genesis_block = GenesisBlock::new(validators, chain_config, vec![1, 2, 3, 4]);

    let block0 = genesis_block.to_block();
    db.insert_block(block0.clone()).unwrap();

    // Create second block
    let header = BlockHeader {
        version: 1,
        height: 1,
        previous_block_hash: block0.hash(),
        timestamp: 1_234_567_890,
        state_root: [3u8; 32],
        proposer: vec![4, 5, 6],
    };

    let body = BlockBody::new(vec![]);
    let block1 = Block { header, body };

    db.insert_block(block1.clone()).unwrap();

    // Verify both blocks can be retrieved
    let block0_retrieved = db.get_block_by_height(0).unwrap().unwrap();
    let block1_retrieved = db.get_block_by_height(1).unwrap().unwrap();

    assert_eq!(block0_retrieved.header.height, 0);
    assert_eq!(block1_retrieved.header.height, 1);

    // Verify tip is updated to block1
    let tip_hash = db.get_tip_block_hash().unwrap().unwrap();
    assert_eq!(tip_hash, block1.hash());
}

#[test]
fn test_insert_and_get_deposit_intent() {
    let (db, _temp_dir) = create_test_db();

    let tracking_id = Uuid::new_v4().to_string();
    let intent = DepositIntent {
        amount_sat: 50000,
        deposit_tracking_id: tracking_id.clone(),
        deposit_address: "test_deposit_address".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "test_user_pubkey".to_string(),
    };

    // Insert the intent
    let insert_result = db.insert_deposit_intent(intent);
    assert!(insert_result.is_ok());

    // Test get by tracking ID
    let get_by_id = db.get_deposit_intent(&tracking_id);
    assert!(get_by_id.is_ok());
    let retrieved_intent = get_by_id.unwrap().unwrap();
    assert_eq!(retrieved_intent.amount_sat, 50000);
    assert_eq!(retrieved_intent.deposit_address, "test_deposit_address");

    // Test get by address
    let get_by_address = db.get_deposit_intent_by_address("test_deposit_address");
    assert!(get_by_address.is_ok());
    let retrieved_intent = get_by_address.unwrap().unwrap();
    assert_eq!(retrieved_intent.amount_sat, 50000);
    assert_eq!(retrieved_intent.deposit_tracking_id, tracking_id);
}

#[test]
fn test_get_deposit_intent_nonexistent() {
    let (db, _temp_dir) = create_test_db();

    let result = db.get_deposit_intent("nonexistent_id");
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());

    let result = db.get_deposit_intent_by_address("nonexistent_address");
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_get_all_deposit_intents_empty() {
    let (db, _temp_dir) = create_test_db();

    let result = db.get_all_deposit_intents();
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

#[test]
fn test_get_all_deposit_intents_multiple() {
    let (db, _temp_dir) = create_test_db();

    let intent1 = DepositIntent {
        amount_sat: 10000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "address1".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "user1".to_string(),
    };

    let intent2 = DepositIntent {
        amount_sat: 20000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "address2".to_string(),
        timestamp: 1_234_567_891,
        user_pubkey: "user2".to_string(),
    };

    // Insert both intents
    db.insert_deposit_intent(intent1).unwrap();
    db.insert_deposit_intent(intent2).unwrap();

    // Get all intents
    let all_intents = db.get_all_deposit_intents().unwrap();
    assert_eq!(all_intents.len(), 2);

    // Verify both intents are present (order not guaranteed)
    let addresses: Vec<String> = all_intents
        .iter()
        .map(|i| i.deposit_address.clone())
        .collect();
    assert!(addresses.contains(&"address1".to_string()));
    assert!(addresses.contains(&"address2".to_string()));
}

#[test]
fn test_deposit_intent_address_lookup_independence() {
    let (db, _temp_dir) = create_test_db();

    let intent = DepositIntent {
        amount_sat: 30000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "unique_address".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "user".to_string(),
    };

    db.insert_deposit_intent(intent.clone()).unwrap();

    // Both lookup methods should return the same intent
    let by_id = db
        .get_deposit_intent(&intent.deposit_tracking_id)
        .unwrap()
        .unwrap();
    let by_address = db
        .get_deposit_intent_by_address("unique_address")
        .unwrap()
        .unwrap();

    assert_eq!(by_id.amount_sat, by_address.amount_sat);
    assert_eq!(by_id.deposit_tracking_id, by_address.deposit_tracking_id);
    assert_eq!(by_id.deposit_address, by_address.deposit_address);
}

#[test]
fn test_complex_chain_state_serialization() {
    let (db, _temp_dir) = create_test_db();

    // Create complex chain state
    let mut accounts = HashMap::new();
    accounts.insert("addr1".to_string(), Account::new("addr1".to_string(), 1000));
    accounts.insert("addr2".to_string(), Account::new("addr2".to_string(), 2000));
    accounts.insert("addr3".to_string(), Account::new("addr3".to_string(), 3000));

    let mut chain_state = ChainState::new_with_accounts(accounts, 42);

    // Add deposit intents
    let intent1 = DepositIntent {
        amount_sat: 5000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "deposit1".to_string(),
        timestamp: 1_234_567_890,
        user_pubkey: "user1".to_string(),
    };

    let intent2 = DepositIntent {
        amount_sat: 6000,
        deposit_tracking_id: Uuid::new_v4().to_string(),
        deposit_address: "deposit2".to_string(),
        timestamp: 1_234_567_891,
        user_pubkey: "user2".to_string(),
    };

    chain_state.insert_deposit_intent(intent1);
    chain_state.insert_deposit_intent(intent2);

    // Store and retrieve
    db.insert_chain_state(chain_state.clone()).unwrap();
    let retrieved_state = db.get_chain_state().unwrap().unwrap();

    // Verify accounts
    assert_eq!(retrieved_state.get_account("addr1").unwrap().balance, 1000);
    assert_eq!(retrieved_state.get_account("addr2").unwrap().balance, 2000);
    assert_eq!(retrieved_state.get_account("addr3").unwrap().balance, 3000);

    // Verify deposit intents
    let deposit_intents = retrieved_state.get_all_deposit_intents();
    assert_eq!(deposit_intents.len(), 2);

    // Verify block height
    assert_eq!(retrieved_state.get_block_height(), 42);
}

#[test]
fn test_error_handling_corrupted_data() {
    let (db, _temp_dir) = create_test_db();

    // Manually insert corrupted data
    let corrupted_data = b"corrupted_bincode_data";
    db.db
        .put_cf(
            db.db.cf_handle("deposit_intents").unwrap(),
            "corrupted_intent",
            corrupted_data,
        )
        .unwrap();

    // Attempt to retrieve should return error
    let result = db.get_deposit_intent("corrupted_intent");
    assert!(result.is_err());
}
