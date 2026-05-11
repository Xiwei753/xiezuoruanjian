#!/bin/bash
set -e

echo "=== Validating Workspace Fixture ==="
python3 tools/validate_workspace.py fixtures/sample_workspace

echo "=== Testing Rust Core ==="
cd core/writer_core
cargo fmt --check || echo "Warning: cargo fmt failed, you may need to format code"
cargo check
cargo test
cd ../..

echo "=== Checking Linux Native Skeleton ==="
./tools/build_linux_native.sh

echo "=== Done ==="
