#!/bin/bash
# =============================================================================
# HarmonyOS / OHOS 构建脚本
# =============================================================================
#
# 构建 Rust writer_core FFI 库并复制到 HarmonyOS 应用的 prebuilt 目录。
#
# 使用方法：
#   ./build_harmony.sh
#
# 环境要求：
#   - Rust 工具链 (rustup, cargo)
#   - OHOS NDK (需要设置 OHOS_NDK_HOME 环境变量)
#   - 目标已安装: rustup target add aarch64-unknown-linux-ohos
#
# 构建产物：
#   - Rust FFI .so: apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so
#
# 注意事项：
#   - 当前仅支持 arm64-v8a 架构
#   - 需要 OHOS SDK/NDK 提供 ohos-clang 交叉编译链接器
#   - 如果 OHOS NDK 未配置，脚本将给出提示并退出

set -euo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"
PREBUILT_DIR="$WORKSPACE_ROOT/apps/harmony/entry/src/main/prebuilt/arm64-v8a"

echo "=== 素笺写作 HarmonyOS FFI 构建脚本 ==="
echo ""

# 检查 OHOS NDK
if [ -z "${OHOS_NDK_HOME:-}" ]; then
    echo "警告: OHOS_NDK_HOME 环境变量未设置。"
    echo ""
    echo "如果已安装 OHOS SDK，请设置："
    echo "  export OHOS_NDK_HOME=/path/to/ohos-sdk/ohos-llvm"
    echo ""
    echo "将尝试使用系统已安装的目标进行构建..."
fi

# 检查 Rust 目标是否已安装
if ! rustup target list | grep -q "aarch64-unknown-linux-ohos (installed)"; then
    echo "目标 aarch64-unknown-linux-ohos 未安装。"
    echo "请运行: rustup target add aarch64-unknown-linux-ohos"
    echo ""
    echo "如果此目标不可用，请确保已添加 OHOS 目标："
    echo "  rustup target add aarch64-unknown-linux-ohos"
    exit 1
fi

# 创建 prebuilt 目录
mkdir -p "$PREBUILT_DIR"

echo "清理旧的 FFI 库..."
rm -f "$PREBUILT_DIR/libwriter_core_ffi.so"

echo "使用 cargo 编译 aarch64-unknown-linux-ohos 目标..."
cd "$WORKSPACE_ROOT/core/writer_core"

# 如果 OHOS_NDK_HOME 已设置，配置链接器
if [ -n "${OHOS_NDK_HOME:-}" ]; then
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="${OHOS_NDK_HOME}/bin/ohos-clang"
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_AR="${OHOS_NDK_HOME}/bin/llvm-ar"
    echo "  链接器: ${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER}"
    echo "  AR: ${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_AR}"
fi

cargo build --target aarch64-unknown-linux-ohos --release --features harmony-ffi

if [ $? -ne 0 ]; then
    echo "错误: Rust FFI 库编译失败。"
    exit 1
fi

# 复制 .so 到 prebuilt 目录
SO_PATH="$WORKSPACE_ROOT/target/aarch64-unknown-linux-ohos/release/libwriter_core.so"

if [ ! -f "$SO_PATH" ]; then
    echo "错误: 找不到编译产物 $SO_PATH"
    echo "请检查 cargo build 是否成功完成。"
    exit 1
fi

cp "$SO_PATH" "$PREBUILT_DIR/libwriter_core_ffi.so"

if [ ! -f "$PREBUILT_DIR/libwriter_core_ffi.so" ]; then
    echo "错误: 复制 libwriter_core_ffi.so 到 prebuilt 目录失败。"
    exit 1
fi

echo ""
echo "=== 构建成功 ==="
echo "  FFI 库: $PREBUILT_DIR/libwriter_core_ffi.so"
echo "  大小: $(ls -lh "$PREBUILT_DIR/libwriter_core_ffi.so" | awk '{print $5}')"
echo ""
echo "下一步: 在 DevEco Studio 中构建 HarmonyOS 应用"
