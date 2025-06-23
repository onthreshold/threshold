echo "Running integration tests..."
docker compose down
docker compose up -d --build

echo "Waiting for nodes to start..."
sleep 5

echo "Running tests..."
cargo run --bin integration-tests -- test --amount 5000