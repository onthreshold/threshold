#!/bin/bash

# Set the deposit amount
deposit_amount=$1
fee=$2

# Check if the deposit amount is provided
if [ -z "$deposit_amount" ]; then
  echo "Error: Please provide a deposit amount."
  exit 1
fi

# Check if the fee is provided
if [ -z "$fee" ]; then
  fee=200
  echo "Fee not provided, using default value: $fee"
fi

# Run the deposit command and extract the deposit_address
deposit_output=$(cargo run --bin "cli" -- deposit "tb1q62qxecgfyn7ud6esrxc50xh9hs56dysatwqheh" "$deposit_amount" 2>&1)
echo "$deposit_output"
deposit_address=$(echo "$deposit_output" | grep "deposit_address" | sed 's/.*deposit_address: "\([^"]*\)".*/\1/')

# Check if deposit_address is empty
if [ -z "$deposit_address" ]; then
  echo "Error: Could not extract deposit_address from deposit command output."
  exit 1
fi

echo "Deposit Address: $deposit_address"

# Run the utxo-spend command with the extracted address and extract the transaction_id
  utxo_spend_output=$(cargo run --bin "utxo-spend" -- "$deposit_address" "$deposit_amount" "$fee" 2>&1)

  echo "$utxo_spend_output"
  transaction_id=$(echo "$utxo_spend_output" | grep "Broadcast Transaction txid:" | sed 's/.*Broadcast Transaction txid: //')

  # Check if transaction_id is empty
  if [ -z "$transaction_id" ]; then
    echo "Error: Could not extract transaction_id from utxo-spend command output."
    exit 1
  fi

  # Print the transaction ID
  echo "Submitted Transaction ID: $transaction_id"
