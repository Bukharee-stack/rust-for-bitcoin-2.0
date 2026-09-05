#!/bin/bash
# Bitcoin Wallet Wrapper Script
# This ensures environment variables are loaded correctly

# Load environment variables
export BITCOIN_NETWORK=regtest
export BITCOIN_RPC_URL=http://127.0.0.1:18443
export BITCOIN_RPC_USER=bitcoinrpc
export BITCOIN_RPC_PASSWORD=password
export WALLET_DIR=./wallet_data

# Run the wallet
cargo run --release -- "$@"
