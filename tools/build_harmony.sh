#!/bin/bash
# =============================================================================
# HarmonyOS 构建脚本
# =============================================================================
#
# 交叉编译 Rust writer_core 的 HarmonyOS C-ABI FFI 库，并复制到 HarmonyOS
# 应用的 prebuilt 目录：
#   apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so
#
# MacBERT4CSC 模型由 entry 模块的 Hvigor 构建钩子在 HAP 资源打包前获取并
# 校验（见 apps/harmony/entry/hvigorfile.ts）；本脚本还负责获取鸿蒙 arm64
# 的 ONNX Runtime 并写入 prebuilt 目录，避免把 Android/Linux 的 so 冒充
# 鸿蒙库。两者都使用持久缓存，重复构建不会重新下载。
#
# 使用方法：
#   ./tools/build_harmony.sh
#
# HarmonyOS Native SDK 探测顺序：
#   1. $OHOS_NDK_HOME（用户覆盖项；指向 HarmonyOS Native SDK 目录）
#   2. $HARMONY_CLI_HOME/sdk/default/openharmony/native
#      （仓库标准入口 tools/setup_harmony_cli.sh 的安装位置，默认 ~/.harmony-cli；
#        CI 的 HarmonyOS Static Analysis 也走这条）
#   3. /opt/devecostudio/sdk/default/openharmony/native 等 DevEco Studio 安装位置
#
# 注：SDK 目录命名 openharmony/native 是工具链事实（HarmonyOS Native 构建链
# 使用的目录命名），不代表产品兼容 OpenHarmony。本客户端只做 HarmonyOS。
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
# 1. 定位 HarmonyOS Native SDK 目录
# -----------------------------------------------------------------------------
detect_native_dir() {
    local -a cands=()
    # 与 hvigor 使用同一份 SDK：tools/setup_harmony_cli.sh 将此变量设为
    # command-line-tools/sdk，DevEco 的 CLI 构建也以它作为 SDK 根目录。
    if [ -n "${DEVECO_SDK_HOME:-}" ]; then
        cands+=("$DEVECO_SDK_HOME/default/openharmony/native")
        cands+=("$DEVECO_SDK_HOME/openharmony/native")
    fi
    [ -n "${OHOS_NDK_HOME:-}" ] && cands+=("$OHOS_NDK_HOME")
    # 仓库标准入口 tools/setup_harmony_cli.sh 的安装位置（默认 ~/.harmony-cli）
    local cli_home="${HARMONY_CLI_HOME:-$HOME/.harmony-cli}"
    cands+=("$cli_home/sdk/default/openharmony/native")
    # DevEco Studio 图形安装的位置，作为兜底
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
    echo "错误：未找到 HarmonyOS Native SDK（clang）。" >&2
    echo "" >&2
    echo "先按仓库标准入口安装 HarmonyOS 命令行工具链（默认装到 ~/.harmony-cli）：" >&2
    echo "  source tools/setup_harmony_cli.sh" >&2
    echo "" >&2
    echo "若你用的是别的安装位置，用环境变量直接指定 Native SDK 目录：" >&2
    echo "  export OHOS_NDK_HOME=<你的路径>/openharmony/native" >&2
    echo "" >&2
    echo "注：路径中的 openharmony/native 是工具链目录命名，不代表产品兼容 OpenHarmony。" >&2
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

echo "HarmonyOS Native SDK : $NATIVE_DIR"
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
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="-C link-arg=--target=$CLANG_TARGET -C link-arg=--sysroot=$SYSROOT -C link-arg=-Wl,-soname,libwriter_core_ffi.so"

# -----------------------------------------------------------------------------
# 4. 从仓库准备鸿蒙目标 ONNX Runtime（按 manifest 校验 SHA-256）
# -----------------------------------------------------------------------------
mkdir -p "$PREBUILT_DIR"
echo "准备 OHOS arm64 ONNX Runtime ..."
python3 "$WORKSPACE_ROOT/crates/sujian-proof/tools/fetch_model.py" \
    --runtime harmony-arm64 \
    --runtime-only \
    --runtime-output "$PREBUILT_DIR"
if [ ! -f "$PREBUILT_DIR/libonnxruntime.so" ]; then
    echo "错误：未得到鸿蒙 libonnxruntime.so。" >&2
    exit 1
fi
file "$PREBUILT_DIR/libonnxruntime.so" 2>/dev/null || true

# -----------------------------------------------------------------------------
# 5. 构建 cdylib 并复制到 prebuilt 目录
# -----------------------------------------------------------------------------

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

# Issue #1007：记录实际编译进 .so 的 Git revision。Hvigor 的诊断 build_identity
# 优先读这个文件，避免只重建 HAP 时把新 HEAD 误当成新的 Rust 内核版本。
CORE_REVISION="unknown"
if command -v git >/dev/null 2>&1; then
    CORE_REVISION="$(git -C "$WORKSPACE_ROOT" rev-parse --short HEAD 2>/dev/null || true)"
    if [ -n "$CORE_REVISION" ]; then
        if [ -n "$(git -C "$WORKSPACE_ROOT" status --porcelain --untracked-files=no 2>/dev/null || true)" ]; then
            CORE_REVISION="${CORE_REVISION}-dirty"
        fi
    else
        CORE_REVISION="unknown"
    fi
fi
printf '%s\n' "$CORE_REVISION" > "$PREBUILT_DIR/writer_core_revision.txt"

if [ ! -f "$PREBUILT_DIR/libwriter_core_ffi.so" ]; then
    echo "错误：复制 libwriter_core_ffi.so 到 prebuilt 目录失败。" >&2
    exit 1
fi

# ONNX Runtime 的 DT_NEEDED 包含 libc++_shared.so。准备 SDK 匹配的 runtime
# 并审计刚生成的 FFI 与 ONNX 的 DT_NEEDED / 动态未定义符号提供方；CMake
# 完成 writer_core_napi 链接后还会对最终 NAPI 文件运行同一套诊断。
bash "$WORKSPACE_ROOT/tools/prepare_harmony_native_runtime.sh" "$NATIVE_DIR" "$PREBUILT_DIR"

echo ""
echo "=== 构建成功 ==="
echo "  FFI 库: $PREBUILT_DIR/libwriter_core_ffi.so"
echo "  FFI revision: $CORE_REVISION"
ls -lh "$PREBUILT_DIR/libwriter_core_ffi.so"
file "$PREBUILT_DIR/libwriter_core_ffi.so" 2>/dev/null || true
echo ""
echo "下一步: 运行 Hvigor 构建 HarmonyOS 应用"
