#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

set -e

echo "=== Validating Workspace Fixture ==="
python3 tools/validate_workspace.py fixtures/sample_workspace

echo "=== Testing Rust Workspace ==="
cargo fmt --all --check || { echo "cargo fmt failed"; exit 1; }
cargo check --workspace
cargo test --workspace

echo "=== Checking Linux Native Skeleton ==="
./tools/build_linux.sh

echo "=== Checking Android Debug Build ==="
#./tools/build_android.sh

echo "=== Done ==="
