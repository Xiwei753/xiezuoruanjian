#!/bin/bash
set -e

echo "=== Validating Workspace Fixture ==="
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
python3 "$DIR/validate_workspace.py" "$DIR/../fixtures/sample_workspace"

echo "=== Testing Rust Core ==="
cd "$DIR/../core/writer_core"
cargo fmt --check
cargo check
cargo test


echo "=== Checking Linux Native Skeleton ==="
"$DIR/build_linux_native.sh"

echo "=== Done ==="
