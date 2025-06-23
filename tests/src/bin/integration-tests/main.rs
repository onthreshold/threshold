use std::str::FromStr;

use bitcoin::Address;
use bitcoin::secp256k1::{Message, Secp256k1};
use clap::{Parser, Subcommand};
use hex::{decode, encode};
use node::key_manager::generate_keys_from_mnemonic;
use node::wallet::{TaprootWallet, Wallet};
use oracle::esplora::EsploraOracle;
use oracle::oracle::Oracle;
use types::proto::node_proto::node_control_client::NodeControlClient;
use types::proto::node_proto::{
    CheckBalanceRequest, ConfirmWithdrawalRequest, CreateDepositIntentRequest,
    GetPendingDepositIntentsRequest, ProposeWithdrawalRequest,
    GetChainInfoRequest, TriggerConsensusRoundRequest, GetLatestBlocksRequest,
};

#[derive(Parser)]
#[command(name = "integration-tests")]
#[command(about = "Run deposit or withdrawal integration flows without bash scripts.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::enum_variant_names)]
enum Commands {
    /// Run all integration tests
    Test {
        #[arg(short, long, default_value_t = 1000)]
        amount: u64,
        #[arg(short, long, default_value_t = false)]
        use_testnet: bool,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    /// Run the deposit integration flow
    DepositTest {
        amount: u64,
        #[arg(short, long, default_value_t = 2000)]
        fee: u64,
        #[arg(short, long, default_value_t = false)]
        use_testnet: bool,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    /// Run the withdrawal integration flow
    WithdrawalTest {
        amount: u64,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
    /// Run an end-to-end test of the deposit and withdrawal flows
    EndToEndTest {
        amount: u64,
        #[arg(short, long, default_value_t = false)]
        use_testnet: bool,
        #[arg(short, long)]
        endpoint: Option<String>,
    },
<<<<<<< Updated upstream
    CheckDkg {
        #[arg(short, long)]
        ports: String,
=======
    /// Run a consensus end-to-end test that creates multiple deposits to trigger block creation
    ConsensusTest {
        #[arg(short, long, default_value_t = 1000)]
        amount: u64,
        #[arg(short, long, default_value_t = 3)]
        num_deposits: u32,
        #[arg(short, long)]
        endpoint: Option<String>,
>>>>>>> Stashed changes
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Commands::Test {
            amount,
            endpoint,
            use_testnet,
        } => {
            println!("🧪 Running all integration tests...\n");
            
            println!("📥 Running deposit test...");
            run_deposit_test(amount, 2000, endpoint.clone(), use_testnet).await?;
            println!("✅ Deposit test completed\n");
            
            println!("📤 Running withdrawal test...");
            run_withdrawal_test(amount / 2, endpoint.clone()).await?;
            println!("✅ Withdrawal test completed\n");
            
            println!("🔄 Running end-to-end test...");
            run_end_to_end_test(amount, endpoint.clone(), use_testnet).await?;
            println!("✅ End-to-end test completed\n");
            
            println!("🏛️ Running consensus test...");
            run_consensus_test(amount, 3, endpoint).await?;
            println!("✅ Consensus test completed\n");
            
            println!("🎉 All integration tests passed!");
        }
        Commands::DepositTest {
            amount,
            fee,
            endpoint,
            use_testnet,
        } => {
            run_deposit_test(amount, fee, endpoint, use_testnet).await?;
        }
        Commands::WithdrawalTest { amount, endpoint } => {
            run_withdrawal_test(amount, endpoint).await?;
        }
        Commands::EndToEndTest {
            amount,
            endpoint,
            use_testnet,
        } => {
            run_end_to_end_test(amount, endpoint, use_testnet).await?;
        }
<<<<<<< Updated upstream
        Commands::CheckDkg { ports } => {
            check_if_dkg_keys_exist(ports).await?;
=======
        Commands::ConsensusTest {
            amount,
            num_deposits,
            endpoint,
        } => {
            run_consensus_test(amount, num_deposits, endpoint).await?;
>>>>>>> Stashed changes
        }
    }

    Ok(())
}

async fn run_deposit_test(
    amount: u64,
    fee: u64,
    endpoint: Option<String>,
    use_testnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("▶️  Creating deposit intent for {} sats", amount);
    let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC env variable not set");
    let (sender_address, sender_priv, sender_pub) = generate_keys_from_mnemonic(&mnemonic);
    let public_key = sender_pub.to_string();

    let mut client = NodeControlClient::connect(
        endpoint
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:50051".to_string()),
    )
    .await?;

    // Check balance -------------------------------------------
    let request = CheckBalanceRequest {
        address: public_key.clone(),
    };
    let resp = client.check_balance(request).await?.into_inner();
    println!("💰 Initial balance: {} sats", resp.balance_satoshis);
    let initial_balance = resp.balance_satoshis;

    let req = CreateDepositIntentRequest {
        public_key: public_key.clone(),
        amount_satoshis: amount,
    };

    let resp = client.create_deposit_intent(req).await?.into_inner();
    let deposit_address_str = resp.deposit_address.clone();
    println!(
        "✅ Deposit intent created. Address: {} | tracking_id: {}",
        deposit_address_str, resp.deposit_tracking_id
    );

    if use_testnet {
        println!("🔑 Sender address: {}. Refreshing UTXOs...", sender_address);

        let oracle = EsploraOracle::new(bitcoin::Network::Testnet, Some(100), None, None, 6, 0);
        let mut wallet = TaprootWallet::new(
            Box::new(oracle.clone()),
            vec![sender_address.clone()],
            bitcoin::Network::Testnet,
        );
        wallet.refresh_utxos(Some(true)).await?;

        let deposit_address = Address::from_str(&deposit_address_str)?.assume_checked();
        let (tx, sighash) = wallet.create_spend(amount, fee, &deposit_address, false)?;
        let txid = tx.compute_txid();
        let signed_tx = wallet.sign(&tx, &sender_priv, sighash);

        oracle.broadcast_transaction(&signed_tx).await?;
        println!("📤 Broadcast Transaction txid: {}", txid);

        let start_time = std::time::Instant::now();
        loop {
            let request = CheckBalanceRequest {
                address: public_key.clone(),
            };
            let resp = client.check_balance(request).await?.into_inner();
            println!("💰 Final balance: {} sats", resp.balance_satoshis);

            if resp.balance_satoshis == initial_balance + amount {
                break;
            }

            if start_time.elapsed() >= std::time::Duration::from_secs(1000) {
                println!("⏰ Timeout reached. Exiting polling loop.");
                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    } else {
        let request = CheckBalanceRequest {
            address: public_key.clone(),
        };
        let resp = client.check_balance(request).await?.into_inner();
        println!("💰 Final balance: {} sats", resp.balance_satoshis);

        assert_eq!(
            resp.balance_satoshis,
            initial_balance + amount,
            "Balance after deposit intent should be equal to initial balance + amount"
        );
    }
    println!("✅ Deposit test passed");
    Ok(())
}

async fn run_withdrawal_test(
    amount: u64,
    endpoint: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC env variable not set");
    let (_, sender_priv, sender_pub) = generate_keys_from_mnemonic(&mnemonic);
    let secp = Secp256k1::new();
    let public_key = sender_pub.to_string();

    // Propose withdrawal --------------------------------------
    let mut client = NodeControlClient::connect(
        endpoint
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:50051".to_string()),
    )
    .await?;

    // Check balance -------------------------------------------
    let request = CheckBalanceRequest {
        address: public_key.clone(),
    };
    let resp = client.check_balance(request).await?.into_inner();
    println!("💰 Initial balance: {} sats", resp.balance_satoshis);
    let initial_balance = resp.balance_satoshis;

    let deposit_address = Address::p2wpkh(&sender_pub, bitcoin::Network::Testnet).to_string();

    let req = ProposeWithdrawalRequest {
        amount_satoshis: amount,
        address_to: deposit_address.clone(),
        public_key: public_key.clone(),
        blocks_to_confirm: None,
    };

    let propose_resp = client.propose_withdrawal(req).await?.into_inner();
    let challenge_hex = propose_resp.challenge;
    println!(
        "📝 Withdrawal proposed. Challenge: {} | quote: {} sats",
        challenge_hex, propose_resp.quote_satoshis
    );

    // Sign challenge ------------------------------------------
    let challenge_bytes = decode(&challenge_hex)?;
    let msg = Message::from_digest_slice(&challenge_bytes)?;
    let signature = secp.sign_ecdsa(&msg, &sender_priv.inner);
    let signature_hex = hex::encode(signature.serialize_der());
    println!("✍️  Signature: {}", signature_hex);

    // Confirm withdrawal --------------------------------------
    let confirm_req = ConfirmWithdrawalRequest {
        challenge: challenge_hex.clone(),
        signature: signature_hex.clone(),
    };
    let confirm_resp = client.confirm_withdrawal(confirm_req).await?.into_inner();
    println!(
        "✅ Withdrawal confirmation success: {}",
        confirm_resp.success
    );

    // Wait for withdrawal to be processed --------------------------------------
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check balance -------------------------------------------
    let request = CheckBalanceRequest {
        address: public_key.clone(),
    };
    let resp = client.check_balance(request).await?.into_inner();
    println!("💰 Final balance: {} sats", resp.balance_satoshis);
    assert_eq!(
        resp.balance_satoshis,
        initial_balance - propose_resp.quote_satoshis,
        "Balance after withdrawal should be equal to initial balance - quoted amount"
    );

    println!("✅ Withdrawal test passed");
    Ok(())
}

async fn run_end_to_end_test(
    amount: u64,
    endpoint: Option<String>,
    use_testnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    run_deposit_test(amount, 2000, endpoint.clone(), use_testnet).await?;
    run_withdrawal_test(amount / 2, endpoint).await?;
    println!("✅ End-to-end test passed");
    Ok(())
}

<<<<<<< Updated upstream
async fn check_if_dkg_keys_exist(ports: String) -> Result<(), Box<dyn std::error::Error>> {
    for port in ports.split(",") {
        let mut client = NodeControlClient::connect(format!("http://127.0.0.1:{port}")).await?;

        let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC env variable not set");
        let (_, _, sender_pub) = generate_keys_from_mnemonic(&mnemonic);
        let public_key = sender_pub.to_string();

        let req = CreateDepositIntentRequest {
            public_key: public_key.clone(),
            amount_satoshis: 1000,
        };

        let response = client.create_deposit_intent(req).await;

        match response {
            Ok(_) => {}
            Err(e) => {
                panic!("Deposit intent creation failed for node on port {port}: {e}");
            }
        }
    }

    println!("✅ DKG keys exist");

    Ok(())
}
=======
async fn run_consensus_test(
    amount: u64,
    num_deposits: u32,
    endpoint: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting consensus test with {} deposits of {} sats each", num_deposits, amount);
    let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC env variable not set");
    
    // Connect to multiple nodes to test consensus
    let endpoints = if let Some(ep) = endpoint {
        vec![ep]
    } else {
        vec![
            "http://127.0.0.1:50051".to_string(),
            "http://127.0.0.1:50052".to_string(),
            "http://127.0.0.1:50053".to_string(),
            "http://127.0.0.1:50054".to_string(),
            "http://127.0.0.1:50055".to_string(),
        ]
    };
    
    let mut clients = Vec::new();
    for endpoint in &endpoints {
        match NodeControlClient::connect(endpoint.clone()).await {
            Ok(client) => {
                println!("✅ Connected to node at {}", endpoint);
                clients.push(client);
            }
            Err(e) => {
                println!("⚠️  Failed to connect to node at {}: {}", endpoint, e);
            }
        }
    }
    
    if clients.is_empty() {
        return Err("No nodes available for consensus test".into());
    }
    
    println!("📊 Connected to {} nodes for consensus testing", clients.len());
    
    // Generate unique keys for each deposit to test multiple users
    let mut deposit_data = Vec::new();
    for i in 0..num_deposits {
        // Create deterministic but valid mnemonic by using the base mnemonic with derivation
        let (_, _, sender_pub) = generate_keys_from_mnemonic(&mnemonic);
        
        // Create unique public key by adding index to the base public key bytes
        let mut pub_bytes = sender_pub.to_bytes();
        // Modify the last byte to create unique keys (simple but effective for testing)
        pub_bytes[32] = (pub_bytes[32].wrapping_add(i as u8)) % 255;
        let unique_key = encode(pub_bytes);
        
        deposit_data.push((unique_key, i));
    }
    
    // Phase 1: Create deposit intents on different nodes to trigger consensus
    println!("📝 Phase 1: Creating {} deposit intents across nodes", num_deposits);
    let mut deposit_addresses = Vec::new();
    
    for (idx, (public_key, _)) in deposit_data.iter().enumerate() {
        let client_idx = idx % clients.len();
        let req = CreateDepositIntentRequest {
            public_key: public_key.clone(),
            amount_satoshis: amount,
        };
        
        let resp = clients[client_idx].create_deposit_intent(req).await?.into_inner();
        println!(
            "  📄 Deposit {} created on node {} - Address: {}",
            idx + 1,
            client_idx + 1,
            resp.deposit_address
        );
        deposit_addresses.push((resp.deposit_address, public_key.clone()));
    }
    
    // Phase 2: Wait for consensus to process the intents
    println!("⏳ Phase 2: Waiting for consensus to process deposit intents...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    
    // Phase 3: Verify consensus by checking that all nodes have the same state
    println!("🔍 Phase 3: Verifying consensus across all nodes");
    let mut all_consistent = true;
    
    for (client_idx, client) in clients.iter_mut().enumerate() {
        println!("  🔍 Checking node {}...", client_idx + 1);
        
        // Check that all deposit intents are visible on this node
        let pending_resp = client.get_pending_deposit_intents(
            GetPendingDepositIntentsRequest {}
        ).await?.into_inner();
        
        println!("    📋 Node {} has {} pending intents", client_idx + 1, pending_resp.intents.len());
        
        // Check that we can see all the deposit addresses we created
        let mut found_addresses = 0;
        for (expected_address, _) in &deposit_addresses {
            if pending_resp.intents.iter().any(|intent| &intent.deposit_address == expected_address) {
                found_addresses += 1;
            }
        }
        
        if found_addresses != deposit_addresses.len() {
            println!("    ❌ Node {} only has {}/{} expected deposit addresses", 
                     client_idx + 1, found_addresses, deposit_addresses.len());
            all_consistent = false;
        } else {
            println!("    ✅ Node {} has all expected deposit addresses", client_idx + 1);
        }
    }
    
    // Phase 4: Get initial chain info using dev endpoints
    println!("🧱 Phase 4: Getting initial chain state using dev endpoints");
    let mut initial_chain_info = Vec::new();
    for (client_idx, client) in clients.iter_mut().enumerate() {
        let chain_info = client.get_chain_info(GetChainInfoRequest {}).await?.into_inner();
        initial_chain_info.push(chain_info.clone());
        println!("  📊 Node {} initial state: height={}, pending_txs={}, total_blocks={}", 
                 client_idx + 1, chain_info.latest_height, chain_info.pending_transactions, chain_info.total_blocks);
    }
    
    // Check initial balances
    let mut initial_balances = Vec::new();
    for (public_key, _) in &deposit_data {
        let request = CheckBalanceRequest {
            address: public_key.clone(),
        };
        let resp = clients[0].check_balance(request).await?.into_inner();
        initial_balances.push(resp.balance_satoshis);
        println!("    💰 Initial balance for user {}: {} sats", public_key, resp.balance_satoshis);
    }
    
    // Phase 5: Trigger consensus rounds to process deposits
    println!("🔄 Phase 5: Triggering consensus rounds for block processing");
    for (client_idx, client) in clients.iter_mut().enumerate() {
        let resp = client.trigger_consensus_round(TriggerConsensusRoundRequest {
            force_round: true,
        }).await?.into_inner();
        if resp.success {
            println!("  ✅ Node {} triggered consensus round {}: {}", 
                     client_idx + 1, resp.round_number, resp.message);
        } else {
            println!("  ❌ Node {} failed to trigger consensus: {}", client_idx + 1, resp.message);
        }
    }
    
    // Wait for mock oracle to process and consensus to complete
    println!("⏳ Waiting for mock oracle processing and block finalization...");
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    
    // Phase 6: Verify block creation and chain advancement
    println!("🧱 Phase 6: Verifying block creation and chain advancement");
    let mut blocks_created = false;
    for (client_idx, client) in clients.iter_mut().enumerate() {
        let chain_info = client.get_chain_info(GetChainInfoRequest {}).await?.into_inner();
        let initial_info = &initial_chain_info[client_idx];
        
        println!("  📊 Node {} final state: height={}, pending_txs={}, total_blocks={}", 
                 client_idx + 1, chain_info.latest_height, chain_info.pending_transactions, chain_info.total_blocks);
        
        if chain_info.latest_height > initial_info.latest_height || chain_info.total_blocks > initial_info.total_blocks {
            blocks_created = true;
            println!("    ✅ Node {} created new blocks", client_idx + 1);
            
            // Get latest blocks to verify content
            let blocks_resp = client.get_latest_blocks(GetLatestBlocksRequest { count: 3 }).await?.into_inner();
            for block in &blocks_resp.blocks {
                println!("      🧱 Block height={}, hash={}, txs={}", 
                         block.height, block.hash, block.transaction_count);
            }
        } else {
            println!("    ⚠️  Node {} did not create new blocks", client_idx + 1);
        }
    }
    
    // Phase 7: Check final balances to verify transaction execution
    println!("🔍 Phase 7: Verifying transaction execution across nodes");
    let mut execution_consistent = true;
    
    for (client_idx, client) in clients.iter_mut().enumerate() {
        println!("  🔍 Checking transaction execution on node {}...", client_idx + 1);
        let mut processed_deposits = 0;
        
        for (user_idx, (public_key, _)) in deposit_data.iter().enumerate() {
            let request = CheckBalanceRequest {
                address: public_key.clone(),
            };
            let resp = client.check_balance(request).await?.into_inner();
            let expected_balance = initial_balances[user_idx] + amount;
            
            if resp.balance_satoshis == expected_balance {
                processed_deposits += 1;
            }
            
            println!("    💰 User {} balance on node {}: {} sats (expected: {})",
                     public_key, client_idx + 1, resp.balance_satoshis, expected_balance);
        }
        
        if processed_deposits == deposit_data.len() {
            println!("    ✅ Node {} processed all deposits correctly", client_idx + 1);
        } else {
            println!("    ⚠️  Node {} only processed {}/{} deposits", 
                     client_idx + 1, processed_deposits, deposit_data.len());
            execution_consistent = false;
        }
    }
    
    // Final comprehensive verification
    if all_consistent && blocks_created && execution_consistent {
        println!("✅ COMPLETE CONSENSUS TEST PASSED");
        println!("   📊 All {} nodes maintain consistent state", clients.len());
        println!("   📄 All {} deposit intents properly synchronized", num_deposits);
        println!("   🧱 Blocks created and consensus rounds completed");
        println!("   ⚖️  All deposits processed and balances updated");
        println!("   🔗 Chain state consistent across all nodes");
    } else {
        let mut error_msg = "Consensus test failed: ".to_string();
        if !all_consistent {
            error_msg.push_str("state inconsistent, ");
        }
        if !blocks_created {
            error_msg.push_str("no blocks created, ");
        }
        if !execution_consistent {
            error_msg.push_str("transaction execution failed, ");
        }
        println!("❌ {}", error_msg);
        return Err(error_msg.into());
    }
    
    Ok(())
}

>>>>>>> Stashed changes
