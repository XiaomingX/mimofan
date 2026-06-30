#!/bin/bash
set -e

# Define script directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Step 1: Compiling mimofan-cli and mimofan-tui in release mode ==="
cargo build --release -p mimofan-cli -p mimofan

echo "=== Step 2: Deploying and Verifying Binaries ==="
# Verify they exist under target/release/
if [ ! -f "target/release/mimofan" ]; then
    echo "Error: target/release/mimofan binary not found!"
    exit 1
fi

if [ ! -f "target/release/mimofan-tui" ]; then
    echo "Error: target/release/mimofan-tui binary not found!"
    exit 1
fi

echo "Success: Both mimofan and mimofan-tui binaries are successfully deployed at target/release/"

echo "=== Step 3: Executing Test Query ==="
# Execute the non-interactive query flow with custom provider configurations
./target/release/mimofan \
  --provider anthropic \
  --model mimo-v2.5-pro \
  --api-key sk-c7dfzu1r64ii50v9li06avlfw5396yeywbz0olztv2e8m6xk \
  --base-url https://api.xiaomimimo.com/anthropic \
  exec "你好，你是谁，给一句话自我介绍"

echo -e "\n=== All Steps Completed Successfully ==="
