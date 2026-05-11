#!/bin/bash
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

set -e
cd core/writer_core
cargo build --release
echo "Core built successfully"
