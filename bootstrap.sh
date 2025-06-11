#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# Bootstrap multiple TheVault nodes for local testing / development.
#
# This script will:
#   1. Generate N node identities & config files via the CLI (`cargo run --bin cli setup`).
#   2. Extract each nodes public peer-id and configuration file path from the CLI
#      output.
#   3. Update every node's config so that their `allowed_peers` section contains
#      the other N-1 nodes (name: node_X, public_key: <peer_id>).
#
# The script is **non-interactive**.  Provide the password that should be used to
# encrypt the private keys through the KEY_PASSWORD environment variable.  If
# the variable is not set the script will abort (otherwise the CLI would fall
# back to prompting which breaks automation).
#
# Usage example:
#   KEY_PASSWORD=supersecret ./bootstrap.sh          # generates 5 nodes
#   KEY_PASSWORD=secret ./bootstrap.sh 8             # generates 8 nodes
# -----------------------------------------------------------------------------

set -euo pipefail

# --------------------------- Configuration -----------------------------------
NUM_NODES=${1:-5}              # default to 5 nodes if not given as argument
BASE_DIR="visuals/nodes"              # where generated node folders will live
CLI_CMD=(cargo run --quiet --bin cli)   # command to invoke the CLI
# gRPC port to start from. Each node gets BASE_GRPC_PORT + (index-1)
BASE_GRPC_PORT=50051

# Ensure KEY_PASSWORD is provided for non-interactive operation
if [[ -z "${KEY_PASSWORD:-}" ]]; then
  echo "ERROR: Please provide the password via KEY_PASSWORD environment variable." >&2
  exit 1
fi

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# Determine the project root (directory containing Cargo.toml).  In most setups
# the script itself sits at the root, but we fall back to the parent directory
# just in case.
if [[ -f "$SCRIPT_DIR/Cargo.toml" ]]; then
  PROJECT_ROOT="$SCRIPT_DIR"
elif [[ -f "$SCRIPT_DIR/../Cargo.toml" ]]; then
  PROJECT_ROOT="$(realpath "$SCRIPT_DIR/..")"
else
  echo "ERROR: Could not locate Cargo.toml relative to $SCRIPT_DIR" >&2
  exit 1
fi

cd "$PROJECT_ROOT"

mkdir -p "$BASE_DIR"

# Arrays to capture peer ids and config paths (1-indexed for convenience)
declare -a PEER_IDS
declare -a CONFIG_PATHS
declare -a KEY_PATHS
declare -a NODE_DIRS

# --------------------------- Generation loop ---------------------------------
echo "Generating $NUM_NODES nodes…"
for i in $(seq 1 "$NUM_NODES"); do
  node_name="node_${i}"
  node_dir="$BASE_DIR/$node_name"
  mkdir -p "$node_dir"

  echo "  → $node_name (directory: $node_dir)"

  # Run the setup command and capture its stdout (stderr is forwarded directly).
  OUTPUT=$(KEY_PASSWORD="$KEY_PASSWORD" \
           "${CLI_CMD[@]}" setup \
             --output-dir "$node_dir" \
             --file-name "$node_name")

  # Show the CLI output so the user can see what happened.
  echo "$OUTPUT"

  # Extract the peer-id and config path from the output.
  # Expected line format (from CLI):
  #   "Key data has been saved to … with the peer id <PEER_ID>. To modify … here: <CONFIG_PATH>"
  PEER_ID=$(echo "$OUTPUT" | sed -n -E 's/.*peer id ([A-Za-z0-9]+).*/\1/p')
  CONFIG_PATH=$(echo "$OUTPUT" | sed -n -E 's/.*config file here: (.*)$/\1/p')

  if [[ -z "$PEER_ID" || -z "$CONFIG_PATH" ]]; then
    echo "Failed to parse peer-id or config path from CLI output, aborting." >&2
    exit 1
  fi

  PEER_IDS[$i]="$PEER_ID"
  CONFIG_PATHS[$i]="$CONFIG_PATH"
  KEY_PATHS[$i]="$node_dir/${node_name}.json"
  NODE_DIRS[$i]="$node_dir"

done

echo "\nAll peer IDs:"
for i in $(seq 1 "$NUM_NODES"); do
  echo "  node_$i → ${PEER_IDS[$i]}"
done

# --------------------------- Patching configs --------------------------------

echo "\nPatching each node's allowed_peers list…"

# Prepare a comma-separated list of peer ids to pass into Python
ALL_PEERS=$(IFS=','; echo "${PEER_IDS[*]}")

for i in $(seq 1 "$NUM_NODES"); do
  CONFIG_PATH="${CONFIG_PATHS[$i]}"
  echo "  • Updating $(basename "$CONFIG_PATH")"

  # Call an embedded Python snippet to update the YAML config safely.
  CONFIG_PATH="$CONFIG_PATH" \
  SELF_INDEX="$i" \
  NUM_NODES="$NUM_NODES" \
  ALL_PEERS="$ALL_PEERS" \
  python3 - <<'PY'
import os, pathlib, yaml, sys

config_path = pathlib.Path(os.environ['CONFIG_PATH'])
self_index   = int(os.environ['SELF_INDEX'])
all_peers    = os.environ['ALL_PEERS'].split(',')
num_nodes    = int(os.environ['NUM_NODES'])

# Load existing YAML (can be empty)
try:
    data = yaml.safe_load(config_path.read_text()) or {}
except FileNotFoundError:
    sys.stderr.write(f"Config file not found: {config_path}\n")
    sys.exit(1)

# Build the allowed_peers list excluding our own peer id
allowed = []
for idx, peer_id in enumerate(all_peers, start=1):
    if idx == self_index:
        continue
    allowed.append({'name': f'node_{idx}', 'public_key': peer_id})

data['allowed_peers'] = allowed

# Preserve other top-level keys (log_file_path, key_file_path, …)
config_path.write_text(yaml.safe_dump(data, sort_keys=False))
PY

done

echo "\nBootstrap finished successfully!  Node directories are in $BASE_DIR."

# --------------------------- Launch nodes in separate terminals --------------

echo "\nLaunching each node in its own terminal window…"

# Helper: open a new terminal window running the given command string
open_terminal() {
  local cmd="$1"

  if command -v gnome-terminal >/dev/null 2>&1; then
    # GNOME Terminal (default on Pop!_OS / Ubuntu GNOME)
    gnome-terminal -- bash -c "$cmd" &
  elif command -v x-terminal-emulator >/dev/null 2>&1; then
    # Debian alternatives system
    x-terminal-emulator -e bash -c "$cmd" &
  elif command -v konsole >/dev/null 2>&1; then
    konsole --noclose -e bash -c "$cmd" &
  elif command -v xterm >/dev/null 2>&1; then
    xterm -hold -e "$cmd" &
  else
    echo "WARNING: No supported graphical terminal emulator found. Running in background within current shell."
    bash -c "$cmd" &
  fi
}

for i in $(seq 1 "$NUM_NODES"); do
  node_dir="${NODE_DIRS[$i]}"
  node_name="node_${i}"
  manifest_path="$PROJECT_ROOT/Cargo.toml"
  grpc_port=$((BASE_GRPC_PORT + i - 1))
  node_cmd="cd \"$node_dir\" && KEY_PASSWORD=\"$KEY_PASSWORD\" cargo run --quiet --manifest-path \"$manifest_path\" --bin cli run --key-file-path \"${node_name}.json\" --config-file-path \"${node_name}.yaml\" --grpc-port ${grpc_port}; exec bash"

  echo "  → Starting node_$i on gRPC port ${grpc_port} in new window"
  open_terminal "$node_cmd"
done

echo "All nodes launched."
