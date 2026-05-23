#!/usr/bin/env bash
set -euo pipefail

# Style settings
export QT_QUICK_CONTROLS_STYLE=Basic

# Debug settings
export WRITER_DEBUG=1
export WRITER_DEBUG_QML=1
export WRITER_DEBUG_MODULES="${WRITER_DEBUG_MODULES:-app,workspace,tree,project,volume,chapter,sync,settings}"
export WRITER_DEBUG_LEVEL="${WRITER_DEBUG_LEVEL:-info}"
export WRITER_DEBUG_QT_VERBOSE="${WRITER_DEBUG_QT_VERBOSE:-0}"
export RUST_BACKTRACE=full
export RUST_LOG="${RUST_LOG:-warn}"

# Enable QML/Quick/QPA warnings and debug according to QT_VERBOSE settings
if [ -z "${QT_LOGGING_RULES:-}" ]; then
    if [ "${WRITER_DEBUG_QT_VERBOSE:-0}" = "1" ]; then
        export QT_LOGGING_RULES="*.debug=true;qt.quick.hover.trace=false;qt.scenegraph.renderloop=false;qt.quick.mouse.target=false;qt.quick.mouse=false;qt.qml.warning=true;*.warning=true;*.critical=true"
    else
        export QT_LOGGING_RULES="*.debug=false;qt.quick.hover.trace=false;qt.scenegraph.renderloop=false;qt.quick.mouse.target=false;qt.quick.mouse=false;qt.quick.dirty=false;qt.scenegraph.time.*=false;qt.qml.warning=true;*.warning=true;*.critical=true"
    fi
fi

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

generate_summary() {
    # Generate logs/latest-summary.txt
    SUMMARY_FILE="logs/latest-summary.txt"
    echo "=== Debug Summary ===" > "$SUMMARY_FILE"
    echo "Generated at: $(date)" >> "$SUMMARY_FILE"
    echo "Log file path: $LOG_FILE" >> "$SUMMARY_FILE"
    echo "WRITER_DEBUG_MODULES: $WRITER_DEBUG_MODULES" >> "$SUMMARY_FILE"
    echo "WRITER_DEBUG_LEVEL: $WRITER_DEBUG_LEVEL" >> "$SUMMARY_FILE"
    echo "" >> "$SUMMARY_FILE"

    echo "--- Staged WriterDebug and critical/warning/error logs ---" >> "$SUMMARY_FILE"
    if [ -f "$LOG_FILE" ]; then
        grep -E -i "\[WriterDebug\]|error|warn|critical|conflict|failed|success" "$LOG_FILE" | grep -Ev "qt\.quick|qt\.scenegraph|qt\.qpa|\[Qt DEBUG\]" >> "$SUMMARY_FILE" || true
        echo "" >> "$SUMMARY_FILE"

        echo "--- Last 200 lines of non-Qt-debug output ---" >> "$SUMMARY_FILE"
        grep -Ev "qt\.quick|qt\.scenegraph|qt\.qpa|\[Qt DEBUG\]" "$LOG_FILE" | tail -n 200 >> "$SUMMARY_FILE" || true
    else
        echo "Log file not found." >> "$SUMMARY_FILE"
    fi

    echo ""
    echo "[start-debug] Run completed. Logs saved to: $LOG_FILE"
    echo "[start-debug] Summary generated at: $SUMMARY_FILE"
}

trap generate_summary EXIT

echo "=== Debug Configuration ==="
echo "WRITER_DEBUG: $WRITER_DEBUG"
echo "WRITER_DEBUG_MODULES: $WRITER_DEBUG_MODULES"
echo "WRITER_DEBUG_LEVEL: $WRITER_DEBUG_LEVEL"
echo "RUST_LOG: $RUST_LOG"
echo "QT_LOGGING_RULES: $QT_LOGGING_RULES"
echo "Log file path: $LOG_FILE"
if [ "${WRITER_DEBUG_QT_VERBOSE:-0}" = "0" ]; then
    echo "Tip: Qt verbose logging is disabled by default to reduce noise. Run with WRITER_DEBUG_QT_VERBOSE=1 to enable it."
fi
echo "==========================="

echo "[start-debug] Building linux package..."
cargo build -p linux

echo "[start-debug] Running linux package with tracing..."
cargo run -p linux 2>&1 | tee "$LOG_FILE"
