#!/usr/bin/env bash
set -euo pipefail

# Style settings
export QT_QUICK_CONTROLS_STYLE=Basic

# Debug settings
export RUST_BACKTRACE=full
export RUST_LOG=trace

# Enable QML/Quick/QPA warnings
export QT_LOGGING_RULES="qml=true;quick=true;qpa=true;*.warning=true"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure logs directory exists
mkdir -p logs

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="logs/linux-debug-${TIMESTAMP}.log"

echo "[start-debug] Building linux package..."
cargo build -p linux

echo "[start-debug] Running linux package with tracing..."
cargo run -p linux 2>&1 | tee "$LOG_FILE"

echo ""
echo "[start-debug] Run completed. Logs saved to: $LOG_FILE"
