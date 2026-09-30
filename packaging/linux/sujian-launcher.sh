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
    echo "realBinary=$(basename "$REAL_BINARY")"
    echo "appimage=${IS_APPIMAGE}"
    echo "--- begin real binary output ---"
} > "$LATEST_LOG"
cp "$LATEST_LOG" "$LOG_FILE"

# 导出本次 session 路径给真实 ELF，让 Rust startup recorder 复用同一份 history，
# 不再新建第二份 history，也不覆盖 latest.log（统一 session）。
export SUJIAN_STARTUP_SESSION_LOG="$LOG_FILE"
export SUJIAN_STARTUP_LATEST_LOG="$LATEST_LOG"
export SUJIAN_STARTED_BY_LAUNCHER=1

# history 轮转：保留最近 30 份 startup-*.log。
find "$HISTORY_DIR" -maxdepth 1 -name "startup-*.log" -type f -printf '%T@ %p\n' 2>/dev/null \
    | sort -rn | tail -n +31 | cut -d' ' -f2- | while IFS= read -r f; do
        [ -f "$f" ] && find "$f" -delete 2>/dev/null || true
    done

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
    # 非 0 退出码 = 失败启动，更新 last_failed.log。正常退出不覆盖。
    if [ "$EXIT_CODE" -ne 0 ]; then
        cp "$LOG_FILE" "$STARTUP_DIR/last_failed.log" 2>/dev/null || true
    fi
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
