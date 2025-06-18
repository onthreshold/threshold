# Threshold

[![cov](https://github.com/onthreshold/threshold/blob/gh-pages/badges/coverage.svg)](https://github.com/devflowinc/threshold/actions)

A decentralized multisig infrastructure for Bitcoin built in Rust.

## Getting Started

The Threshold project includes a command-line interface for managing nodes, keys, and performing operations.

### CLI

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

#### Other Commands

- `spend <amount> <address_to>` - Spend funds to an address
- `start-signing <hex_message>` - Start signing process
- `deposit <public_key> <amount>` - Create a deposit intent
- `get-pending-deposit-intents` - Get pending deposit intents
- `check-balance <address>` - Check balance of an address

## Installation

#### 1. Install pre-requisites

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

#### 2. Install Rust Toolchain

Ensure you have the correct Rust version:

```bash
rustup default 1.87
rustup update
```

#### 3. Bare Metal Compilation

Compile the project directly on your system:

```bash
# Clone the repository
git clone https://github.com/onthreshold/threshold.git
cd threshold

# Build all workspace members
cargo build --workspace
```

#### 4. Run with Docker Compose

Use the provided docker-compose configuration for multi-node setup:

```bash
# Start all nodes
docker-compose up -d --build

# View logs
docker-compose logs -f

# Stop all nodes
docker-compose down
```

## Testing

### Run Tests

```bash
# Run integration tests
cargo test -p integration-tests

# Run all tests
cargo test --workspace
```

### Test Coverage

Generate test coverage report:

```bash
# Install cargo-tarpaulin (if not already installed)
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --workspace --out Html
```

### Run with `n` nodes

The `setup_nodes.sh` script automates the creation of multiple test nodes for development and testing.

```bash
# Generate 5 nodes (default)
./setup_nodes.sh

# Generate custom number of nodes (8 in this example)
./setup_nodes.sh 8
```

#### Output

- `test_artifacts/test_N_nodes/` directory containing:
  - Individual node configurations (`node_1/`, `node_2/`, etc.)
  - Key files (`node_X.json`, `node_X.yaml`)
  - Docker Compose file
  - Database files (`nodedb_X.db`)

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

### Environment Variables

Refer to the [.env.dist](https://github.com/onthreshold/threshold/blob/main/.env.dist) file for the default environment variables.
