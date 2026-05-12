#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

set -e

echo "=== Validating Workspace Fixture ==="
python3 tools/validate_workspace.py fixtures/sample_workspace

echo "=== Testing Rust Core ==="
cd core/writer_core
cargo fmt --check || { echo "cargo fmt failed"; exit 1; }
cargo check
cargo test
cd ../..

echo "=== Checking Linux Native Skeleton ==="
./tools/build_linux.sh

echo "=== Done ==="
