#!/usr/bin/env bash
# sujian-launcher.sh — 素笺写作 Linux 启动器（打包/脚本层）
#
# Issue #803 评论 5904340835：解决"Rust main() 都没进去"的失败诊断。
# 真实 ELF（/usr/libexec/sujian/sujian-linux-qt）的 stdout/stderr 会被
# 镜像到 ~/.sujianxiezuo/diagnostics/startup/ 下的本次启动日志，即使
# 二进制因为缺动态库、Qt platform plugin、ELF loader 等问题在 main()
# 前就退出，错误仍然会留在日志里。
#
# 本脚本自身不解析业务状态，只负责：创建诊断目录、镜像输出、记录退出码。
set -euo pipefail

LAUNCHER_VERSION="1.0"

# --- 解析 HOME（不硬编码 /home/<name>）---
HOME_DIR="${HOME:-}"
if [ -z "$HOME_DIR" ]; then
    HOME_DIR="$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f6 || true)"
fi
if [ -z "$HOME_DIR" ]; then
    echo "[sujian-launcher] ERROR: cannot determine HOME directory" >&2
    exit 1
fi

# --- 诊断目录结构 ---
DIAG_DIR="$HOME_DIR/.sujianxiezuo/diagnostics"
STARTUP_DIR="$DIAG_DIR/startup"
HISTORY_DIR="$STARTUP_DIR/history"
mkdir -p "$HISTORY_DIR" "$DIAG_DIR/crash" "$DIAG_DIR/runtime" "$DIAG_DIR/exports"

# --- 本次启动日志文件名 ---
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
PID="$$"
LOG_FILE="$HISTORY_DIR/startup-${TIMESTAMP}-${PID}.log"
LATEST_LOG="$STARTUP_DIR/latest.log"

# --- 真实二进制路径（允许开发时通过环境变量覆盖）---
REAL_BINARY="${SUJIAN_REAL_BINARY:-/usr/libexec/sujian/sujian-linux-qt}"
IS_APPIMAGE="no"
[ -n "${APPIMAGE:-}" ] && IS_APPIMAGE="yes"

# --- 写启动头（latest.log 覆盖写开头，history 同步）---
{
    echo "=== sujian-launcher startup ==="
    echo "launcherVersion=${LAUNCHER_VERSION}"
    echo "timestamp=${TIMESTAMP}"
    echo "pid=${PID}"
    echo "home=${HOME_DIR}"
    echo "realBinary=${REAL_BINARY}"
    echo "appimage=${IS_APPIMAGE}"
    echo "args=${*:-}"
    echo "--- begin real binary output ---"
} > "$LATEST_LOG"
cp "$LATEST_LOG" "$LOG_FILE"

# --- 退出码记录 ---
EXIT_CODE=0
on_exit() {
    {
        echo "--- end real binary output ---"
        echo "exitCode=${EXIT_CODE}"
        echo "=== launcher exit ==="
    } >> "$LOG_FILE"
    {
        echo "--- end real binary output ---"
        echo "exitCode=${EXIT_CODE}"
        echo "=== launcher exit ==="
    } >> "$LATEST_LOG"
}
trap on_exit EXIT

# --- 检查真实二进制是否存在且可执行 ---
if [ ! -x "$REAL_BINARY" ]; then
    echo "[sujian-launcher] ERROR: real binary not found or not executable: $REAL_BINARY" >&2
    {
        echo "ERROR: real binary not found or not executable: $REAL_BINARY"
    } >> "$LOG_FILE"
    {
        echo "ERROR: real binary not found or not executable: $REAL_BINARY"
    } >> "$LATEST_LOG"
    EXIT_CODE=127
    exit 127
fi

# --- 运行真实二进制，stdout/stderr 镜像到两个日志文件和终端 ---
# 临时禁用 errexit 以便捕获 PIPESTATUS[0]（真实二进制的退出码）。
# 2>&1 把 stderr 合并进管道，确保 stderr 也写入日志（不吞掉，只是镜像）。
set +e
"$REAL_BINARY" "$@" 2>&1 | tee -a "$LOG_FILE" | tee -a "$LATEST_LOG"
EXIT_CODE="${PIPESTATUS[0]}"
set -e

exit "$EXIT_CODE"
