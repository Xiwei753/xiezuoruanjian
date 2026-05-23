#!/usr/bin/env bash
set -euo pipefail

# Check arguments
if [ $# -gt 0 ]; then
    if [ "$1" = "debug" ]; then
        if [ ! -f "./start-debug.sh" ]; then
            echo "Error: start-debug.sh not found." >&2
            exit 1
        fi
        shift
        exec ./start-debug.sh "$@"
    else
        echo "Error: Unknown argument '$1'" >&2
        echo "Usage:" >&2
        echo "  bash start" >&2
        echo "  bash start debug [sync|tree|ui|all|trace|qt]" >&2
        exit 1
    fi
fi

export QT_QUICK_CONTROLS_STYLE=Basic

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "[start] Building linux package..."
cargo build -p linux

echo "[start] Running linux package..."
cargo run -p linux
