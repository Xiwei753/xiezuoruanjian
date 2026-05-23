#!/usr/bin/env bash
set -euo pipefail

# Style settings
export QT_QUICK_CONTROLS_STYLE=Basic

# Debug settings
export WRITER_DEBUG=1
export WRITER_DEBUG_QML=1
export WRITER_DEBUG_MODULES="${WRITER_DEBUG_MODULES:-all}"
export RUST_BACKTRACE=full
export RUST_LOG=trace

# Enable QML/Quick/QPA warnings and debug
export QT_LOGGING_RULES="qml=true;quick=true;qpa=true;*.warning=true;*.debug=true"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure logs directory exists
mkdir -p logs

# Clean up older log files (keep 20 most recent)
if [ -d logs ]; then
    ls -t logs/linux-debug-*.log 2>/dev/null | tail -n +21 | xargs rm -f || true
fi

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="logs/linux-debug-${TIMESTAMP}.log"

echo "=== Debug Configuration ==="
echo "WRITER_DEBUG: $WRITER_DEBUG"
echo "WRITER_DEBUG_MODULES: $WRITER_DEBUG_MODULES"
echo "RUST_LOG: $RUST_LOG"
echo "QT_LOGGING_RULES: $QT_LOGGING_RULES"
echo "Log file path: $LOG_FILE"
echo "==========================="

echo "[start-debug] Building linux package..."
cargo build -p linux

echo "[start-debug] Running linux package with tracing..."
cargo run -p linux 2>&1 | tee "$LOG_FILE"

echo ""
echo "[start-debug] Run completed. Logs saved to: $LOG_FILE"
