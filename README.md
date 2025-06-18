# Threshold

[![cov](https://github.com/onthreshold/threshold/blob/gh-pages/badges/coverage.svg)](https://github.com/devflowinc/threshold/actions)

A decentralized multisig infrastructure for Bitcoin built in Rust.

This project uses Rust version 1.87:

```bash
rustup default 1.87
```

## Table of Contents

- [Node CLI](#node-cli)
- [Compilation](#compilation)
- [Testing](#testing)

## Node CLI

The Threshold project includes a command-line interface for managing nodes, keys, and performing operations.

### CLI Help

```bash
cargo run --bin cli -- --help
```

### Available Commands

#### Setup - Generate Keypair and Configuration

Generate a new keypair and save it to a file:

```bash
cargo run --bin cli setup --help
```

**Options:**

- `-o, --output-dir <OUTPUT_DIR>` - Directory to save the keypair
- `-f, --file-name <FILE_NAME>` - Name for the keypair file

**Example:**

```bash
cargo run --bin cli setup --output-dir ./keys --file-name my_node
```

#### Run - Start a Node

Run the node and connect to the network:

```bash
cargo run --bin cli run --help
```

**Options:**

- `-k, --key-file-path <KEY_FILE_PATH>` - Path to the key file
- `-c, --config-file-path <CONFIG_FILE_PATH>` - Path to the config file
- `-p, --grpc-port <GRPC_PORT>` - gRPC port (default: 50051)
- `-u, --libp2p-udp-port <LIBP2P_UDP_PORT>` - libp2p UDP port
- `-t, --libp2p-tcp-port <LIBP2P_TCP_PORT>` - libp2p TCP port
- `-d, --database-directory <DATABASE_DIRECTORY>` - Database directory
- `-o, --min-signers <MIN_SIGNERS>` - Minimum number of signers
- `-m, --max-signers <MAX_SIGNERS>` - Maximum number of signers
- `-l, --log-file <LOG_FILE>` - Log file path
- `-f, --confirmation-depth <CONFIRMATION_DEPTH>` - Confirmation depth
- `-s, --monitor-start-block <MONITOR_START_BLOCK>` - Starting block for monitoring
- `--use-mock-oracle` - Use mock oracle for testing

**Example:**

```bash
cargo run --bin cli run \
  --key-file-path ./keys/my_node.json \
  --config-file-path ./keys/my_node.yaml \
  --grpc-port 50051 \
  --use-mock-oracle
```

#### Other Commands

- `spend <amount> <address_to>` - Spend funds to an address
- `start-signing <hex_message>` - Start signing process
- `deposit <public_key> <amount>` - Create a deposit intent
- `get-pending-deposit-intents` - Get pending deposit intents
- `check-balance <address>` - Check balance of an address

## Compilation

### Prerequisites

#### System Dependencies (Bare Metal)

Install the required system dependencies:

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libssl-dev \
  libpq-dev \
  g++ \
  curl \
  protobuf-compiler \
  libclang-dev \
  clang \
  librocksdb-dev

# macOS (using Homebrew)
brew install pkg-config openssl postgresql curl protobuf llvm rocksdb

# CentOS/RHEL/Fedora
sudo yum install -y \
  pkg-config \
  openssl-devel \
  postgresql-devel \
  gcc-c++ \
  curl \
  protobuf-compiler \
  clang-devel \
  rocksdb-devel
```

#### Rust Toolchain

Ensure you have the correct Rust version:

```bash
rustup default 1.87
rustup update
```

### Compilation Methods

#### 1. Bare Metal Compilation

Compile the project directly on your system:

```bash
# Clone the repository
git clone <repository-url>
cd threshold

# Build the CLI
cargo build --bin cli

# Build in release mode (optimized)
cargo build --release --bin cli

# Build all workspace members
cargo build --workspace
```

#### 2. Docker Compilation

Build using Docker:

```bash
# Build the Docker image
docker build -t vault-node .

# Run the container
docker run -it vault-node --help
```

#### 3. Docker Compose

Use the provided docker-compose configuration for multi-node setup:

```bash
# Start all nodes
docker-compose up -d

# View logs
docker-compose logs -f

# Stop all nodes
docker-compose down
```

The docker-compose.yaml file includes 5 pre-configured nodes with:

- Individual gRPC ports (50051-50055)
- Separate database files
- Mock oracle for testing
- Network isolation

## Testing

### Running Tests

#### Unit Tests

Run all unit tests:

```bash
# Run all tests in the workspace
cargo test --workspace

# Run tests for a specific crate
cargo test -p node
cargo test -p protocol
cargo test -p types

# Run tests with verbose output
cargo test --workspace -- --nocapture

# Run tests in release mode
cargo test --release --workspace
```

#### Integration Tests

Run integration tests:

```bash
# Run integration tests
cargo test -p tests

# Run specific integration test modules
cargo test -p tests deposit
cargo test -p tests withdrawl
cargo test -p tests dkg
cargo test -p tests signing
```

#### Test Coverage

Generate test coverage report:

```bash
# Install cargo-tarpaulin (if not already installed)
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --workspace --out Html
```

### Node Setup Script

The `setup_nodes.sh` script automates the creation of multiple test nodes for development and testing.

#### Usage

```bash
# Generate 5 nodes (default)
./setup_nodes.sh

# Generate custom number of nodes
./setup_nodes.sh 8
```

#### What the Script Does

1. **Generates Node Identities**: Creates N node keypairs and configuration files
2. **Configures Peering**: Sets up each node to peer with all other nodes
3. **Creates Docker Compose**: Generates a docker-compose.yaml for the nodes
4. **Builds Docker Image**: Builds the vault-node Docker image
5. **Starts the Cluster**: Launches all nodes using Docker Compose

#### Output

The script creates:

- `test_artifacts/test_N_nodes/` directory containing:
  - Individual node configurations (`node_1/`, `node_2/`, etc.)
  - Key files (`node_X.json`, `node_X.yaml`)
  - Docker Compose file
  - Database files (`nodedb_X.db`)

#### Configuration Details

- **gRPC Ports**: 50056-50060 (for 5 nodes)
- **Key Password**: "supersecret" (for testing)
- **Mock Oracle**: Enabled for testing
- **Network**: Isolated bridge network
- **Signer Configuration**:
  - Min signers: 2/3 of total nodes
  - Max signers: Total number of nodes

### Test Artifacts

The testing system creates several artifacts:

- **Database Files**: `nodedb_X.db` - RocksDB databases for each node
- **Key Files**: JSON and YAML configuration files
- **Log Files**: Node operation logs
- **Docker Images**: Built vault-node images

### Cleanup

To clean up test artifacts:

```bash
# Stop and remove Docker containers
docker-compose down

# Remove test artifacts
rm -rf test_artifacts/

# Remove database files
rm -f nodedb_*.db

# Remove Docker images
docker rmi vault-node
```

## Development

### Project Structure

```
├── bin/cli/           # Command-line interface
├── bin/utxo-spend/    # UTXO spending utilities
├── crates/            # Core Rust crates
│   ├── node/          # Node implementation
│   ├── protocol/      # Protocol definitions
│   ├── types/         # Shared types
│   ├── oracle/        # Oracle implementations
│   └── abci/          # ABCI interface
├── tests/             # Integration tests
├── setup_nodes.sh     # Node setup script
├── docker-compose.yaml # Multi-node Docker setup
└── Dockerfile         # Docker build configuration
```

### Key Dependencies

- **libp2p**: Peer-to-peer networking
- **rocksdb**: Database storage
- **bitcoin**: Bitcoin protocol implementation
- **frost-secp256k1**: Threshold signature scheme
- **tonic**: gRPC framework
- **tokio**: Async runtime

### Environment Variables

- `KEY_PASSWORD`: Password for key encryption
- `IS_TESTNET`: Enable testnet mode
- `RUST_LOG`: Logging level (debug, info, warn, error)
