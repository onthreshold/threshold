use std::future::Future;
use std::str::FromStr;
use std::time::{Duration, Instant};

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
    CheckBalanceRequest, ConfirmWithdrawalRequest, CreateDepositIntentRequest, GetChainInfoRequest,
    GetLatestBlocksRequest, ProposeWithdrawalRequest, TriggerConsensusRoundRequest,
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
    CheckDkg {
        #[arg(short, long)]
        port_range: String,
    },
    /// Run a consensus end-to-end test that creates multiple deposits to trigger block creation
    ConsensusTest {
        #[arg(short, long, default_value_t = 1000)]
        amount: u64,
        #[arg(short, long, default_value_t = 3)]
        num_deposits: u32,
        #[arg(short, long)]
        endpoint: Option<String>,
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

            println!("🔄 Running dkg test...");
            check_if_dkg_keys_exist("50051-50055".to_string()).await?;
            println!("✅ DKG test completed\n");

            println!("⏳ Waiting for DKG deposit intents to settle (max 60s)...");

            // Use either provided endpoint or default first node.
            let monitor_endpoint = endpoint
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());

            poll_until(
                || {
                    let monitor_endpoint = monitor_endpoint.clone();
                    async move {
                        let mut client = NodeControlClient::connect(monitor_endpoint).await?;
                        let info = client
                            .get_chain_info(GetChainInfoRequest {})
                            .await?
                            .into_inner();
                        println!(
                            "   📊 pending_txs: {} | latest_height: {}",
                            info.pending_transactions, info.latest_height
                        );
                        Ok(info.pending_transactions == 0)
                    }
                },
                Duration::from_secs(2),
                Duration::from_secs(60),
            )
            .await?;

            println!("📥 Running deposit test...");
            run_deposit_test(amount, 2000, endpoint.clone(), use_testnet).await?;
            println!("✅ Deposit test completed\n");

            println!("📤 Running withdrawal test...");
            run_withdrawal_test(amount / 2, endpoint.clone()).await?;
            println!("✅ Withdrawal test completed\n");

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
        Commands::CheckDkg { port_range } => {
            check_if_dkg_keys_exist(port_range).await?;
        }
        Commands::ConsensusTest {
            amount,
            num_deposits,
            endpoint,
        } => {
            run_consensus_test(amount, num_deposits, endpoint).await?;
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

    // Wait for deposit intent to be processed --------------------------------------
    let resp = client
        .trigger_consensus_round(TriggerConsensusRoundRequest { force_round: true })
        .await?
        .into_inner();
    if resp.success {
        println!(
            "  ✅ Node triggered consensus round {}: {}",
            resp.round_number, resp.message
        );
    } else {
        println!("  ❌ Node failed to trigger consensus: {}", resp.message);
    }
    // Wait until the deposit is reflected in the user's balance instead of
    // sleeping for a fixed amount of time.
    let expected_balance = initial_balance + amount;
    poll_until(
        || {
            let mut client = client.clone();
            let public_key = public_key.clone();
            async move {
                let request = CheckBalanceRequest {
                    address: public_key,
                };
                match client.check_balance(request).await {
                    Ok(r) => {
                        let balance = r.into_inner().balance_satoshis;
                        println!(
                            "💰 Current balance: {} sats (Expected: {})",
                            balance, expected_balance
                        );
                        Ok(balance == expected_balance)
                    }
                    Err(e) => Err(Box::new(e) as Box<dyn std::error::Error>),
                }
            }
        },
        Duration::from_secs(2),
        Duration::from_secs(60),
    )
    .await?;

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

        poll_until(
            || {
                let mut client = client.clone();
                let public_key = public_key.clone();
                async move {
                    let request = CheckBalanceRequest {
                        address: public_key,
                    };
                    match client.check_balance(request).await {
                        Ok(r) => {
                            let balance = r.into_inner().balance_satoshis;
                            println!(
                                "💰 Current balance: {} sats (Expected: {})",
                                balance,
                                initial_balance + amount
                            );
                            Ok(balance == initial_balance + amount)
                        }
                        Err(e) => Err(Box::new(e) as Box<dyn std::error::Error>),
                    }
                }
            },
            Duration::from_secs(60),   // Keep a larger interval for testnet
            Duration::from_secs(1000), // Original long timeout preserved
        )
        .await?;
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
    let resp = client
        .trigger_consensus_round(TriggerConsensusRoundRequest { force_round: true })
        .await?
        .into_inner();
    if resp.success {
        println!(
            "✅ Node triggered consensus round {}: {}",
            resp.round_number, resp.message
        );
    } else {
        println!("  ❌ Node failed to trigger consensus: {}", resp.message);
    }

    // Wait until the withdrawal is reflected in the user's balance.
    let expected_balance = initial_balance - propose_resp.quote_satoshis;
    poll_until(
        || {
            let mut client = client.clone();
            let public_key = public_key.clone();
            async move {
                let request = CheckBalanceRequest {
                    address: public_key,
                };
                match client.check_balance(request).await {
                    Ok(r) => {
                        let balance = r.into_inner().balance_satoshis;
                        println!(
                            "💰 Current balance: {} sats (Expected: {})",
                            balance, expected_balance
                        );
                        Ok(balance == expected_balance)
                    }
                    Err(e) => Err(Box::new(e) as Box<dyn std::error::Error>),
                }
            }
        },
        Duration::from_secs(2),
        Duration::from_secs(60),
    )
    .await?;

    // Final balance check to assert condition
    let request = CheckBalanceRequest {
        address: public_key.clone(),
    };
    let resp = client.check_balance(request).await?.into_inner();
    println!("💰 Final balance: {} sats", resp.balance_satoshis);
    assert_eq!(
        resp.balance_satoshis, expected_balance,
        "Balance after withdrawal should be equal to initial balance - quoted amount"
    );

    println!("✅ Withdrawal test passed");
    Ok(())
}

async fn check_if_dkg_keys_exist(port_range: String) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = port_range.split('-').collect();
    if parts.len() != 2 {
        return Err(
            "Invalid port range format. Expected format: start-end (e.g., 50051-50055)".into(),
        );
    }

    let start_port: u16 = parts[0].parse().map_err(|_| "Invalid start port")?;
    let end_port: u16 = parts[1].parse().map_err(|_| "Invalid end port")?;

    if start_port > end_port {
        return Err("Start port must be less than or equal to end port".into());
    }

    let mut failed_nodes = Vec::new();

    for (index, port) in (start_port..=end_port).enumerate() {
        let node_number = index + 1;
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
            Ok(_) => {
                println!("✅ DKG key exists for node{node_number}");
            }
            Err(e) => {
                println!("❌ DKG key missing for node{node_number}: {e}");
                failed_nodes.push(node_number);
            }
        }
    }

    assert!(
        failed_nodes.is_empty(),
        "DKG keys missing for nodes: {:?}",
        failed_nodes
    );
    println!("✅ All DKG keys exist");

    Ok(())
}

async fn run_consensus_test(
    amount: u64,
    num_deposits: u32,
    endpoint: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🚀 Starting consensus test with {} deposits of {} sats each",
        num_deposits, amount
    );
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

    println!(
        "📊 Connected to {} nodes for consensus testing",
        clients.len()
    );

    let mut deposit_data = Vec::new();
    for i in 0..num_deposits {
        let (_, _, sender_pub) = generate_keys_from_mnemonic(&mnemonic);

        let mut pub_bytes = sender_pub.to_bytes();
        pub_bytes[32] = (pub_bytes[32].wrapping_add(i as u8)) % 255;
        let unique_key = encode(pub_bytes);

        deposit_data.push((unique_key, i));
    }

    println!("🧱 Getting initial chain state using dev endpoints");
    let mut initial_chain_info = Vec::new();
    for (client_idx, client) in clients.iter_mut().enumerate() {
        let chain_info = client
            .get_chain_info(GetChainInfoRequest {})
            .await?
            .into_inner();
        initial_chain_info.push(chain_info.clone());
        println!(
            "  📊 Node {} initial state: height={}, pending_txs={}",
            client_idx + 1,
            chain_info.latest_height,
            chain_info.pending_transactions,
        );
    }

    // Check initial balances
    let mut initial_balances = Vec::new();
    for (public_key, _) in &deposit_data {
        let request = CheckBalanceRequest {
            address: public_key.clone(),
        };
        let resp = clients[0].check_balance(request).await?.into_inner();
        initial_balances.push(resp.balance_satoshis);
        println!(
            "    💰 Initial balance for user {}: {} sats",
            public_key, resp.balance_satoshis
        );
    }

    println!("📝 Creating {} deposit intents across nodes", num_deposits);
    let mut deposit_addresses = Vec::new();

    for (idx, (public_key, _)) in deposit_data.iter().enumerate() {
        let client_idx = idx % clients.len();
        let req = CreateDepositIntentRequest {
            public_key: public_key.clone(),
            amount_satoshis: amount,
        };

        let resp = clients[client_idx]
            .create_deposit_intent(req)
            .await?
            .into_inner();
        println!(
            "  📄 Deposit {} created on node {} - Address: {}",
            idx + 1,
            client_idx + 1,
            resp.deposit_address
        );
        deposit_addresses.push((resp.deposit_address, public_key.clone()));
    }

    // Trigger consensus rounds to process deposits
    println!("🔄 Triggering consensus rounds for block processing");
    for (client_idx, client) in clients.iter_mut().enumerate() {
        let resp = client
            .trigger_consensus_round(TriggerConsensusRoundRequest { force_round: true })
            .await?
            .into_inner();
        if resp.success {
            println!(
                "  ✅ Node {} triggered consensus round {}: {}",
                client_idx + 1,
                resp.round_number,
                resp.message
            );
        } else {
            println!(
                "  ❌ Node {} failed to trigger consensus: {}",
                client_idx + 1,
                resp.message
            );
        }
    }

    // Wait for mock oracle to process and consensus to complete
    println!("⏳ Waiting for mock oracle processing and block finalization (max 60s)...");
    // Poll any node until chain height increases or timeout.
    poll_until(
        || {
            let mut client = clients[0].clone();
            let initial_height = initial_chain_info[0].latest_height;
            async move {
                match client.get_chain_info(GetChainInfoRequest {}).await {
                    Ok(r) => {
                        let info = r.into_inner();
                        Ok(info.latest_height > initial_height)
                    }
                    Err(e) => Err(Box::new(e) as Box<dyn std::error::Error>),
                }
            }
        },
        Duration::from_secs(2),
        Duration::from_secs(60),
    )
    .await?;

    // Verify block creation and chain advancement
    println!("🧱 Verifying block creation and chain advancement");
    let mut blocks_created = false;
    for (client_idx, client) in clients.iter_mut().enumerate() {
        let chain_info = client
            .get_chain_info(GetChainInfoRequest {})
            .await?
            .into_inner();
        let initial_info = &initial_chain_info[client_idx];

        println!(
            "  📊 Node {} final state: height={}, pending_txs={}",
            client_idx + 1,
            chain_info.latest_height,
            chain_info.pending_transactions,
        );

        if chain_info.latest_height > initial_info.latest_height {
            blocks_created = true;
            println!("    ✅ Node {} created new blocks", client_idx + 1);

            // Get latest blocks to verify content
            let blocks_resp = client
                .get_latest_blocks(GetLatestBlocksRequest { count: 3 })
                .await?
                .into_inner();
            for block in &blocks_resp.blocks {
                println!(
                    "      🧱 Block height={}, hash={}, txs={}",
                    block.height, block.hash, block.transaction_count
                );
            }
        } else {
            println!("    ⚠️  Node {} did not create new blocks", client_idx + 1);
        }
    }

    // Check final balances to verify transaction execution
    println!("🔍 Verifying transaction execution across nodes");
    let mut execution_consistent = true;

    for (client_idx, client) in clients.iter_mut().enumerate() {
        println!(
            "  🔍 Checking transaction execution on node {}...",
            client_idx + 1
        );
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

            println!(
                "    💰 User {} balance on node {}: {} sats (expected: {})",
                public_key,
                client_idx + 1,
                resp.balance_satoshis,
                expected_balance
            );
        }

        if processed_deposits == deposit_data.len() {
            println!(
                "    ✅ Node {} processed all deposits correctly",
                client_idx + 1
            );
        } else {
            println!(
                "    ⚠️  Node {} only processed {}/{} deposits",
                client_idx + 1,
                processed_deposits,
                deposit_data.len()
            );
            execution_consistent = false;
        }
    }

    // Final comprehensive verification
    if blocks_created && execution_consistent {
        println!("✅ COMPLETE CONSENSUS TEST PASSED");
        println!(
            "   📊 All {} nodes maintain consistent state",
            clients.len()
        );
        println!(
            "   📄 All {} deposit intents properly synchronized",
            num_deposits
        );
        println!("   ✅ Blocks created and consensus rounds completed");
        println!("   ⚖️  All deposits processed and balances updated");
        println!("   🔗 Chain state consistent across all nodes");
    } else {
        let mut error_msg = "Consensus test failed: ".to_string();
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

// -----------------------------------------------------------------------------
// 🛠️  Utility helpers
// -----------------------------------------------------------------------------
/// Polls the provided asynchronous condition every `poll_interval` until it
/// returns `Ok(true)` or the `timeout` duration is exceeded.
///
/// If the timeout elapses before the condition is met, an error is returned.
async fn poll_until<F, Fut>(
    mut condition_fn: F,
    poll_interval: Duration,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, Box<dyn std::error::Error>>>,
{
    let start = Instant::now();

    loop {
        if condition_fn().await? {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            return Err("Timeout reached while waiting for condition".into());
        }

        tokio::time::sleep(poll_interval).await;
    }
}
