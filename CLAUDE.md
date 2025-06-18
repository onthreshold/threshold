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
docker-compose up

# Build container
docker build -t vault-node .
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
- **gRPC API**: Tonic-based server for deposit/withdrawal operations
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
- **WithdrawlState**: Handles two-phase withdrawals (challenge + confirm)
- **BalanceState**: Tracks user account balances via gRPC CheckBalance RPC
- **DKGState**: Manages distributed key generation

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