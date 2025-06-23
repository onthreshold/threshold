echo "Running integration tests..."
docker compose down
docker compose up -d --build

echo "Waiting for nodes to start..."
sleep 5

echo "Running tests..."
cargo run --bin integration-tests -- end-to-end-test 5000
cargo run --bin integration-tests -- consensus-test --amount 5000 --num-deposits 3