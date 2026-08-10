#!/usr/bin/env bash
# 开发机诊断抓取脚本（Issue #612 六）。
#
# 用法:
#   tools/android/capture_device_diagnostics.sh logcat     # 抓 logcat 快照
#   tools/android/capture_device_diagnostics.sh bugreport  # 抓完整 bugreport
#
# 输出落到 artifacts/android-diagnostics/ 下，文件名带时间戳。
# 需要已通过 adb 连接的开发机（adb devices 能看到设备）。

set -euo pipefail

USAGE="Usage: $0 {logcat|bugreport}"

if [[ $# -ne 1 ]]; then
    echo "$USAGE" >&2
    exit 1
fi

MODE="$1"
OUT_DIR="artifacts/android-diagnostics"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"

case "$MODE" in
    logcat)
        mkdir -p "$OUT_DIR"
        OUT_FILE="$OUT_DIR/logcat-$TIMESTAMP.txt"
        echo "Capturing logcat to $OUT_FILE ..."
        adb logcat -b main,system,crash -v threadtime > "$OUT_FILE"
        echo "Done: $OUT_FILE"
        ;;
    bugreport)
        mkdir -p "$OUT_DIR"
        OUT_FILE="$OUT_DIR/bugreport-$TIMESTAMP.zip"
        echo "Capturing bugreport to $OUT_FILE ..."
        adb bugreport "$OUT_FILE"
        echo "Done: $OUT_FILE"
        ;;
    *)
        echo "$USAGE" >&2
        exit 1
        ;;
esac
