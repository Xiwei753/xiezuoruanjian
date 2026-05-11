#!/bin/bash
set -e
cd core/writer_core
cargo build --release
echo "Core built successfully"
