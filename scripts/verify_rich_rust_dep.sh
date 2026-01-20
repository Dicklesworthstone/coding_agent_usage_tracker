#!/bin/bash
set -euo pipefail

echo "=== Verifying rich_rust dependency ==="

if ! grep -q 'rich_rust' Cargo.toml; then
    echo "ERROR: rich_rust not found in Cargo.toml"
    exit 1
fi

echo "Building project..."
cargo build 2>&1 | head -50

echo "Running smoke tests..."
cargo test rich::tests --lib -- --nocapture

echo "Checking binary size..."
cargo build --release
ls -lh target/release/caut

echo "=== Verification complete ==="
