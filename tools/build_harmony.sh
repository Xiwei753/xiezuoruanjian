#!/bin/bash
# =============================================================================
# HarmonyOS / OHOS 构建脚本
# =============================================================================
#
# 交叉编译 Rust writer_core 的 HarmonyOS C-ABI FFI 库，并复制到 HarmonyOS
# 应用的 prebuilt 目录：
#   apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so
#
# 使用方法：
#   ./tools/build_harmony.sh
#
# OHOS Native SDK 探测顺序：
#   1. $OHOS_NDK_HOME（用户覆盖项；指向 OpenHarmony SDK 的 native 目录）
#   2. /opt/devecostudio/sdk/default/openharmony/native（DevEco Studio 默认安装）
#   3. ~/DevEcoStudio*/sdk/default/openharmony/native 等常见位置
#
# 脚本会基于探测到的 SDK 自动注入 linker / ar / sysroot / C 编译器参数，
# 不依赖系统 PATH 中存在 ohos-clang。
#
# 注意事项：
#   - 当前仅支持 arm64-v8a 架构
#   - 缺少 Rust 目标时自动执行 rustup target add aarch64-unknown-linux-ohos

set -euo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"
PREBUILT_DIR="$WORKSPACE_ROOT/apps/harmony/entry/src/main/prebuilt/arm64-v8a"
RUST_TARGET="aarch64-unknown-linux-ohos"
CLANG_TARGET="aarch64-linux-ohos"

echo "=== 素笺写作 HarmonyOS FFI 构建脚本 ==="
echo ""

# -----------------------------------------------------------------------------
# 1. 定位 OHOS Native SDK 目录
# -----------------------------------------------------------------------------
detect_native_dir() {
    local -a cands=()
    [ -n "${OHOS_NDK_HOME:-}" ] && cands+=("$OHOS_NDK_HOME")
    cands+=(
        "/opt/devecostudio/sdk/default/openharmony/native"
        "$HOME/DevEcoStudio/sdk/default/openharmony/native"
        "$HOME/command-line-tools/sdk/default/openharmony/native"
    )
    shopt -s nullglob
    cands+=("$HOME"/DevEcoStudio*/sdk/default/openharmony/native)
    shopt -u nullglob

    local d
    for d in "${cands[@]}"; do
        [ -n "$d" ] || continue
        if [ -x "$d/llvm/bin/clang" ] || [ -x "$d/bin/clang" ]; then
            printf '%s\n' "$d"
            return 0
        fi
    done
    return 1
}

NATIVE_DIR="$(detect_native_dir)" || {
    echo "错误：未找到 OHOS Native SDK（clang）。" >&2
    echo "" >&2
    echo "请设置环境变量指向 OpenHarmony SDK 的 native 目录后重试：" >&2
    echo "  export OHOS_NDK_HOME=/opt/devecostudio/sdk/default/openharmony/native" >&2
    exit 1
}

LLVM_BIN=""
for d in "$NATIVE_DIR/llvm/bin" "$NATIVE_DIR/bin"; do
    if [ -x "$d/clang" ]; then LLVM_BIN="$d"; break; fi
done

CLANG="$LLVM_BIN/aarch64-unknown-linux-ohos-clang"
[ -x "$CLANG" ] || CLANG="$LLVM_BIN/clang"
LLVM_AR="$LLVM_BIN/llvm-ar"
SYSROOT="$NATIVE_DIR/sysroot"

if [ ! -d "$SYSROOT" ]; then
    echo "错误：sysroot 不存在：$SYSROOT" >&2
    exit 1
fi
if [ ! -x "$CLANG" ] || [ ! -x "$LLVM_AR" ]; then
    echo "错误：$LLVM_BIN 下缺少 clang 或 llvm-ar。" >&2
    exit 1
fi

echo "OHOS Native SDK : $NATIVE_DIR"
echo "clang           : $CLANG"
echo "llvm-ar         : $LLVM_AR"
echo "sysroot         : $SYSROOT"
echo ""

# -----------------------------------------------------------------------------
# 2. 确保 Rust 目标已安装
# -----------------------------------------------------------------------------
if ! rustup target list --installed 2>/dev/null | grep -qx "$RUST_TARGET"; then
    echo "安装 Rust 目标 $RUST_TARGET ..."
    rustup target add "$RUST_TARGET"
fi

# -----------------------------------------------------------------------------
# 3. 注入交叉编译工具链配置
# -----------------------------------------------------------------------------
# linker/ar：rustc 链接 cdylib 与生成静态库使用；
# CC/AR/CFLAGS：cc crate 在编译 vendored C 依赖（libgit2）时使用；
# CARGO_TARGET_*_RUSTFLAGS：仅作用于目标三元组，避免污染 host 构建脚本。
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$CLANG"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_AR="$LLVM_AR"
export CC_aarch64_unknown_linux_ohos="$CLANG"
export AR_aarch64_unknown_linux_ohos="$LLVM_AR"
export CFLAGS_aarch64_unknown_linux_ohos="--target=$CLANG_TARGET --sysroot=$SYSROOT -D__MUSL__"
export CXXFLAGS_aarch64_unknown_linux_ohos="--target=$CLANG_TARGET --sysroot=$SYSROOT -D__MUSL__"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="-C link-arg=--target=$CLANG_TARGET -C link-arg=--sysroot=$SYSROOT"

# -----------------------------------------------------------------------------
# 4. 构建 cdylib 并复制到 prebuilt 目录
# -----------------------------------------------------------------------------
mkdir -p "$PREBUILT_DIR"
rm -f "$PREBUILT_DIR/libwriter_core_ffi.so"

cd "$WORKSPACE_ROOT"
echo "cargo build --release --target $RUST_TARGET -p writer-platform-harmony"
cargo build --release --target "$RUST_TARGET" -p writer-platform-harmony

SO_SRC="$WORKSPACE_ROOT/target/$RUST_TARGET/release/libwriter_platform_harmony.so"
if [ ! -f "$SO_SRC" ]; then
    echo "错误：找不到编译产物 $SO_SRC" >&2
    exit 1
fi

cp "$SO_SRC" "$PREBUILT_DIR/libwriter_core_ffi.so"

if [ ! -f "$PREBUILT_DIR/libwriter_core_ffi.so" ]; then
    echo "错误：复制 libwriter_core_ffi.so 到 prebuilt 目录失败。" >&2
    exit 1
fi

echo ""
echo "=== 构建成功 ==="
echo "  FFI 库: $PREBUILT_DIR/libwriter_core_ffi.so"
ls -lh "$PREBUILT_DIR/libwriter_core_ffi.so"
file "$PREBUILT_DIR/libwriter_core_ffi.so" 2>/dev/null || true
echo ""
echo "下一步: 运行 Hvigor 构建 HarmonyOS 应用"
