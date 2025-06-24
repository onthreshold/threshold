# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust-based threshold cryptography system for Bitcoin operations using FROST (Flexible Round-Optimized Schnorr Threshold signatures). The system implements a distributed key generation protocol with P2P networking for secure multi-party Bitcoin transactions.

## Rust Version Requirement

This project requires Rust 1.87:

```bash
rustup default 1.87
```

## Core Commands

### Development

```bash
# Build all crates
cargo build --all

# Run all tests
cargo test --all

# Run tests for specific crate
cargo test -p abci
cargo test -p node

# Run single test
cargo test test_name

# Format check (required for CI)
cargo fmt --all -- --check

# Linting (CI fails on warnings)
cargo clippy -- -D warnings

# Build specific binary
cargo build --bin cli

# Code coverage analysis
cargo tarpaulin --out html
```

### Node Operations

```bash
# Generate 5-node testnet configuration
KEY_PASSWORD=supersecret ./bootstrap.sh

# Setup individual node
cargo run --bin cli setup

# Start node
cargo run --bin cli run
```

### Docker Development

```bash
# Run 5-node testnet cluster
docker-compose up -d --build

# View logs from all nodes
docker-compose logs

# Stop the cluster
docker-compose down

# Build container
docker build -t vault-node .

# Multi-node DKG testing workflow
# 1. Start n nodes using setup script (max 13 nodes for DKG to work)
./setup_nodes.sh <n>

# 2. Run DKG integration test (note: port range is 50057 to 50057+n-1)
cargo run --bin integration-tests check-dkg --port-range 50057-50063  # for 7 nodes
cargo run --bin integration-tests check-dkg --port-range 50057-50069  # for 13 nodes (max working)

# 3. Clean up containers
docker ps -q | xargs docker stop

# DKG Scalability Limits:
# ✅ Works: 7-13 nodes
# ❌ Shaky: 14+ nodes (DKG process doesn't complete)
# Note: The limit appears to be around 13 nodes but should be 256
```

### Integration Testing

```bash
# End-to-end deposit test (requires docker cluster)
cargo run --bin integration-tests -- end-to-end-test 5000

# Deposit test only
cargo run --bin integration-tests -- deposit-test 1000

# Withdrawal test only
cargo run --bin integration-tests -- withdrawal-test 1000

# Consensus test with multiple deposits
cargo run --bin integration-tests -- consensus-test --amount 1000 --num-deposits 3

# Single node endpoint test
cargo run --bin integration-tests -- deposit-test 1000 --endpoint http://127.0.0.1:50051
```

### Website Development

```bash
cd website/
yarn dev      # Development server
yarn build    # Production build
yarn format   # Prettier formatting
```

## Architecture

### Crate Structure

- **`crates/node/`** - Main node with P2P networking, gRPC server, and handlers
- **`crates/abci/`** - Application Blockchain Interface for chain state management
- **`crates/protocol/`** - Core protocol logic and blockchain definitions
- **`crates/types/`** - Shared type definitions and error handling
- **`crates/oracle/`** - Bitcoin blockchain oracle (Esplora client + mock)

### Key Components

- **FROST Protocol**: Threshold signature implementation using `frost-secp256k1`
- **P2P Networking**: libp2p with Gossipsub, mDNS discovery, TCP/QUIC transports
- **gRPC API**: Tonic-based server for deposit/withdrawal operations, includes dev endpoints for chain inspection
- **Bitcoin Integration**: Taproot wallet with Esplora API client
- **ABCI Layer**: Manages chain state, database operations, and transaction execution

### ABCI Architecture

The ABCI crate implements a clean separation of concerns:

- **`ChainInterface`**: Main trait for blockchain operations (deposit intents, transaction execution, account management)
- **`ChainState`**: In-memory state management with serialization support
- **`TransactionExecutor`**: Stack-based virtual machine for executing protocol transactions
- **`Db` trait**: Database abstraction with RocksDB implementation

### Testing Infrastructure

- **`tests/`** - Separate crate with integration tests using mock implementations
- **`crates/abci/src/tests/`** - Organized unit tests by module:
  - `chain_state.rs` - ChainState and Account tests
  - `executor.rs` - TransactionExecutor and VM tests
  - `db.rs` - Database operation tests
  - `lib_tests.rs` - ChainInterface integration tests
- **MockNodeCluster**: Multi-node test harness with in-memory databases
- **MockOracle**: Simulated blockchain for testing

## Development Patterns

### Generic Design

Code is generic over trait types (`Network`, `Db`, `Oracle`, `ChainInterface`) to enable mocking and testing. Always maintain this abstraction when adding new functionality.

### Handler Architecture

The system uses an actor-like pattern where operations are handled by specific handlers:

- **DepositState**: Manages deposit intents and address derivation
- **WithdrowlState**: Handles two-phase withdrawals (challenge + confirm)
- **BalanceState**: Tracks user account balances via gRPC CheckBalance RPC, also handles dev endpoints (GetChainInfo, GetLatestBlocks)
- **DKGState**: Manages distributed key generation
- **ConsensusState**: Handles FROST consensus protocol, includes TriggerConsensusRound dev endpoint

### Transaction Execution Model

The ABCI transaction executor uses a stack-based virtual machine:

- **Operations**: `OpPush`, `OpCheckOracle`, `OpIncrementBalance`, `OpDecrementBalance`
- **Stack Management**: LIFO operations with type-safe data handling
- **Allowance System**: Oracle validation creates allowances for balance increments
- **Error Handling**: Errors push zero to stack and stop execution

### Idempotent Operations

Functions like `update_user_balance` track processed transaction IDs to prevent double-processing. Maintain this pattern for all state-modifying operations.

### Testing Isolation

Each test creates its own isolated state. Tests are structured as:

1. **Unit tests**: Test individual components in isolation (`crates/*/src/tests/`)
2. **Integration tests**: Test component interactions (`tests/src/`)
3. **End-to-end tests**: Full system workflows via gRPC

## Environment Variables

- `KEY_PASSWORD`: Required for encrypted key operations and bootstrap script
- `IS_TESTNET`: Toggle between development/production modes
- `MNEMONIC`: Test wallet seed phrases for development

## CI/CD Requirements

The GitHub Actions pipeline enforces:

- Code formatting (`cargo fmt --all -- --check`)
- Build success (`cargo build --all`)
- Test passing (`cargo test --all`)
- Clippy linting with zero warnings (`cargo clippy -- -D warnings`)

## Key Files to Understand

### Core Architecture

- **`crates/node/src/lib.rs`** - Defines `NodeState<N,D,O>` generics and registers handlers
- **`crates/abci/src/lib.rs`** - ChainInterface trait and implementation
- **`crates/abci/src/executor.rs`** - Transaction execution virtual machine
- **`crates/abci/src/chain_state.rs`** - In-memory state management

### Infrastructure

- **`crates/node/src/utils/swarm_manager.rs`** - P2P message definitions and gossipsub logic
- **`crates/abci/src/db/rocksdb.rs`** - Persistent database implementation
- **`tests/src/mocks/`** - Mock implementations for testing

### Testing

- **`tests/src/mocks/network.rs`** - MockNodeCluster for multi-node tests
- **`tests/src/mocks/abci.rs`** - Mock ChainInterface for testing
- **`crates/abci/src/tests/`** - Organized unit test modules
- **`tests/src/consensus/`** - Block consensus unit tests
- **`tests/src/bin/integration-tests/`** - End-to-end integration test binary

## Adding New Features

1. **Implement core logic** in appropriate crate (usually `crates/node/` or `crates/abci/`)
2. **Add trait abstractions** for external dependencies to maintain generic design
3. **Create mock implementations** in `tests/src/mocks/` for testing
4. **Add gRPC endpoints** in protobuf definitions if needed
5. **Write comprehensive tests**:
   - Unit tests in the same crate (`src/tests/`)
   - Integration tests in `tests/src/`
   - Use `MockNodeCluster` for multi-node scenarios
6. **Ensure idempotent behavior** for state modifications
7. **Run code coverage** with `cargo tarpaulin` to verify test completeness

## ABCI-Specific Considerations

When working with the ABCI crate:

- **Field visibility**: Some struct fields are `pub(crate)` for testing access
- **Error handling**: Always use `NodeError` for consistent error propagation
- **State persistence**: Changes to `ChainState` must be flushed to database
- **Transaction safety**: Use allowance system for balance increments via oracle validation
- **Test organization**: Unit tests are organized by module in `src/tests/` directory

## Code Maintenance Guidance

- you don't need comments for things you remove or to clarify things. keep comments to a minimum, and don't use them whenever possible

## Development Guidelines

- ensure that you always try to modualarize and try to be DRY as possible. dont modualarize unecessarily but ensure that you can reuse code when possible

## Development Endpoints

The system includes gRPC development endpoints for testing and debugging:

- **GetChainInfo**: Returns chain state information (latest height, pending transactions, etc.)
- **TriggerConsensusRound**: Manually trigger a consensus round (useful for testing)
- **GetLatestBlocks**: Retrieve recent block information

These endpoints are implemented in existing handlers:
- `GetChainInfo` and `GetLatestBlocks` in `BalanceState` handler
- `TriggerConsensusRound` in `ConsensusState` handler

## Integration Test Architecture

The integration test binary (`tests/src/bin/integration-tests/main.rs`) provides comprehensive testing:

1. **Deposit Test**: Creates deposit intents and verifies balance updates
2. **Withdrawal Test**: Tests two-phase withdrawal (propose + confirm)  
3. **End-to-End Test**: Combines deposit and withdrawal flows
4. **Consensus Test**: Multi-node consensus verification with multiple deposits

Tests work with both:
- **Mock Oracle**: For unit/integration testing (instant processing)
- **Real Testnet**: For end-to-end validation (requires actual Bitcoin transactions)

The consensus test specifically verifies:
- State synchronization across all nodes
- Deposit intent propagation via gossipsub
- Block creation and finalization
- Chain state consistency in RocksDB

## Docker Integration

The system supports multi-node testing via Docker Compose:
- 5-node cluster configuration in `docker-compose.yml`
- Automated key generation via `bootstrap.sh`
- gRPC endpoints exposed on ports 50051-50055
- Logs accessible via `docker-compose logs`

Integration tests can target specific nodes or test across the entire cluster for consensus verification.

## Current Implementation Status

### Completed Features

1. **Dev gRPC Endpoints** - ✅ WORKING
   - `GetChainInfo`: Returns chain height, pending transactions, total blocks
   - `TriggerConsensusRound`: Manually triggers consensus rounds
   - `GetLatestBlocks`: Retrieves recent block information
   - All endpoints properly implemented in existing handlers

2. **Enhanced Integration Tests** - ✅ WORKING  
   - Restructured CLI with main `test` command and subcommands
   - Comprehensive consensus test using all 5 nodes
   - Dev endpoint integration for chain state verification
   - Multi-phase testing (state sync, block creation, execution)

3. **Consensus Improvements** - ✅ WORKING
   - Fixed block validation logic (compare transactions, not entire blocks)
   - Fixed 2/3+ threshold calculation for voting
   - Consensus rounds triggering correctly
   - Prevotes and precommits working

### Known Issues

1. **Mock Oracle Transaction Processing** - ❌ NEEDS FIX
   - Mock oracle creates dummy transactions but they don't update balances
   - Consensus can process blocks but transaction execution not completing
   - Even basic deposit tests failing due to balance not updating

2. **Consensus Finalization** - ⚠️ PARTIAL
   - Getting 3/5 precommits but need 4/5 for finalization
   - Block proposals working, voting working, but not reaching final threshold

### Next Steps

1. Fix mock oracle transaction processing to properly update balances
2. Investigate why not all 5 nodes are voting in consensus
3. Ensure finalized blocks actually execute transactions and update chain state

## Development Memory

- when you make changes you have to docker compose down and then docker compose up -d --build to have the latest version of the code running in the test node network
