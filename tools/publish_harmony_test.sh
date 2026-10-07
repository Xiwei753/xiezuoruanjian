#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Repository-standard HarmonyOS CLI/SDK entrypoint. This exports
# DEVECO_SDK_HOME and the matching Node/ohpm/hdc toolchain.
# shellcheck disable=SC1091
source "$ROOT/tools/setup_harmony_cli.sh"

cd "$ROOT"
exec python3 "$ROOT/tools/publish_harmony_test.py" "$@"
