#!/usr/bin/env bash
set -euo pipefail

# Style settings
export QT_QUICK_CONTROLS_STYLE=Basic

show_usage() {
    echo "Usage:"
    echo "  bash start"
    echo "  bash start debug"
    echo "  bash start debug sync"
    echo "  bash start debug trace"
    echo "  bash start debug qt"
}

# Default values if not set in environment
WRITER_DEBUG_MODULES_DEFAULT="app,workspace,tree,project,volume,chapter,sync,settings"
WRITER_DEBUG_LEVEL_DEFAULT="info"
WRITER_DEBUG_QT_VERBOSE_DEFAULT="0"

has_args=false
while [ $# -gt 0 ]; do
    has_args=true
    case "$1" in
        sync)
            export WRITER_DEBUG_MODULES="sync"
            ;;
        tree)
            export WRITER_DEBUG_MODULES="tree,project,volume,chapter,editor"
            ;;
        ui)
            export WRITER_DEBUG_MODULES="app,workspace,tree,settings"
            ;;
        all)
            export WRITER_DEBUG_MODULES="all"
            ;;
        trace)
            export WRITER_DEBUG_LEVEL="trace"
            export WRITER_DEBUG_MODULES="all"
            ;;
        qt)
            export WRITER_DEBUG_QT_VERBOSE="1"
            ;;
        *)
            echo "Error: Unknown debug parameter '$1'" >&2
            show_usage >&2
            exit 1
            ;;
    esac
    shift
done

# If no arguments or if variables not specified, use defaults but do NOT override already set env variables
export WRITER_DEBUG_MODULES="${WRITER_DEBUG_MODULES:-$WRITER_DEBUG_MODULES_DEFAULT}"
export WRITER_DEBUG_LEVEL="${WRITER_DEBUG_LEVEL:-$WRITER_DEBUG_LEVEL_DEFAULT}"
export WRITER_DEBUG_QT_VERBOSE="${WRITER_DEBUG_QT_VERBOSE:-$WRITER_DEBUG_QT_VERBOSE_DEFAULT}"

export WRITER_DEBUG=1
export WRITER_DEBUG_QML=1
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

prepend_path_var() {
    local var_name="$1"
    local path_value="$2"
    local current_value="${!var_name:-}"
    case ":$current_value:" in
        *":$path_value:"*) ;;
        "::") export "$var_name=$path_value" ;;
        *) export "$var_name=$path_value:$current_value" ;;
    esac
}

detect_qt6_header_version() {
    local header="/usr/include/qt6/QtCore/qtcoreversion.h"
    local line version
    [ -f "$header" ] || return 1
    while IFS= read -r line; do
        case "$line" in
            *QTCORE_VERSION_STR*)
                version="${line#*\"}"
                version="${version%%\"*}"
                [[ "$version" == 6.* ]] || return 1
                printf '%s\n' "$version"
                return 0
                ;;
        esac
    done < "$header"
    return 1
}

detect_qt6_qmake() {
    local candidates=(
        "${QMAKE:-}"
        "/usr/lib64/qt6/bin/qmake"
        "/usr/lib64/qt6/bin/qmake6"
        "/usr/bin/qmake6"
        "/usr/bin/qmake-qt6"
        "qmake6"
        "qmake-qt6"
    )
    local candidate version
    for candidate in "${candidates[@]}"; do
        [ -n "$candidate" ] || continue
        if version="$($candidate -query QT_VERSION 2>/dev/null)" && [[ "$version" == 6.* ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

if QMAKE_DETECTED="$(detect_qt6_qmake)"; then
    export QMAKE="$QMAKE_DETECTED"
    export QT_VERSION_MAJOR=6
    QT_VERSION_DETECTED="$($QMAKE -query QT_VERSION 2>/dev/null || true)"
elif QT_VERSION_DETECTED="$(detect_qt6_header_version)"; then
    export QT_INCLUDE_PATH="${QT_INCLUDE_PATH:-/usr/include/qt6}"
    export QT_LIBRARY_PATH="${QT_LIBRARY_PATH:-/usr/lib64}"
    export QT_VERSION_MAJOR=6
else
    QT_VERSION_DETECTED="unknown"
fi

prepend_path_var QML2_IMPORT_PATH "/usr/lib64/qt6/qml"
prepend_path_var QML_IMPORT_PATH "/usr/lib64/qt6/qml"
prepend_path_var QT_PLUGIN_PATH "/usr/lib64/qt6/plugins"

# Ensure logs directory exists and is writable
mkdir -p logs
if [ ! -w logs ]; then
    echo "Error: Logs directory 'logs/' is not writable." >&2
    exit 1
fi

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
echo "Launch mode: debug"
echo "Debug modules: $WRITER_DEBUG_MODULES"
echo "Debug level: $WRITER_DEBUG_LEVEL"
echo "RUST_LOG: $RUST_LOG"
echo "Qt verbose: $( [ "${WRITER_DEBUG_QT_VERBOSE:-0}" = "1" ] && echo "enabled" || echo "disabled" )"
echo "QT_LOGGING_RULES: $QT_LOGGING_RULES"
echo "Qt version detected: $QT_VERSION_DETECTED"
echo "Qt C++ standard: -std=c++17"
echo "QMAKE: ${QMAKE:-not found}"
echo "QT_INCLUDE_PATH: ${QT_INCLUDE_PATH:-}"
echo "QT_LIBRARY_PATH: ${QT_LIBRARY_PATH:-}"
echo "QML2_IMPORT_PATH: ${QML2_IMPORT_PATH:-}"
echo "QT_PLUGIN_PATH: ${QT_PLUGIN_PATH:-}"
echo "QtQuick.Window qmldir: $( [ -f /usr/lib64/qt6/qml/QtQuick/Window/qmldir ] && echo found || echo missing )"
echo "QtQuick Controls qmldir: $( [ -f /usr/lib64/qt6/qml/QtQuick/Controls/qmldir ] && echo found || echo missing )"
echo "Log file path: $LOG_FILE"
if [ "${WRITER_DEBUG_QT_VERBOSE:-0}" = "0" ]; then
    echo "Tip: Qt verbose logging is disabled by default to reduce noise. Run with WRITER_DEBUG_QT_VERBOSE=1 to enable it."
fi
echo "==========================="

echo "[start-debug] Building linux package..."
cargo build -p linux

echo "[start-debug] Running linux package with tracing..."
cargo run -p linux 2>&1 | tee "$LOG_FILE"
