#!/usr/bin/env bash
set -euo pipefail

export QT_QUICK_CONTROLS_STYLE=Basic

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "[start] Building linux package..."
cargo build -p linux

echo "[start] Running linux package..."
cargo run -p linux
