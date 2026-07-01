#!/bin/bash
set -e

# Define script directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Step 1: Compiling mimofan-cli and mimofan-tui in release mode ==="
# 优化编译速度：在本地测试脚本中临时关闭 LTO，并最大化并行代码生成单元，启用 sccache 缓存
CARGO_PROFILE_RELEASE_LTO=false \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=256 \
RUSTC_WRAPPER=sccache \
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

echo "Success: Both mimofan and mimofan-tui binaries are successfully built at target/release/"

echo "=== Step 2.5: Installing to system (~/.cargo/bin) ==="
mkdir -p ~/.cargo/bin
cp target/release/mimofan ~/.cargo/bin/
cp target/release/mimofan-tui ~/.cargo/bin/
echo "Successfully installed mimofan and mimofan-tui to ~/.cargo/bin/"

echo "=== Step 2.8: Configuring ~/.mimofan/settings.json ==="
mkdir -p ~/.mimofan
if [ ! -f ~/.mimofan/settings.json ]; then
    cat << 'EOF' > ~/.mimofan/settings.json
{
  "language": "Chinese"
}
EOF
    echo "Created default ~/.mimofan/settings.json"
else
    echo "~/.mimofan/settings.json already exists. Skipping creation."
fi

echo "=== Step 3: Executing Test Query ==="
# Test the binary execution without making a real API call to avoid 404 errors with placeholder URLs
./target/release/mimofan --version

echo -e "\n=== All Steps Completed Successfully ==="
