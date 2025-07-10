#!/bin/bash

# Extract wallet mnemonic and get complete wallet information
cd /home/runner/workspace

echo "=== QTC WALLET INFORMATION ==="
echo

echo "1. Wallet Address:"
./target/debug/qtcd --data-dir qtc-data wallet list

echo
echo "2. Current Blockchain Height:"
./target/debug/qtcd --data-dir qtc-data chain info

echo
echo "3. Wallet Balance:"
./target/debug/qtcd --data-dir qtc-data wallet balance 2

echo
echo "4. Attempting to get mnemonic phrase..."
echo "mnemonic" | ./target/debug/qtcd --data-dir qtc-data wallet export 2 2>&1 | grep -A 20 "phrase\|words\|mnemonic"

echo
echo "5. Mining a test block to verify functionality..."
./target/debug/qtcd --data-dir qtc-data mine single --address qtc1GLTSTVsFpJjUBQN2nRdZwFzdT436VetwQ --timeout 30

echo
echo "6. Updated blockchain info:"
./target/debug/qtcd --data-dir qtc-data chain info

echo
echo "7. Updated wallet balance:"
./target/debug/qtcd --data-dir qtc-data wallet balance 2