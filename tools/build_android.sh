#!/bin/bash
set -euo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"

VALID_ABIS=("arm64-v8a" "x86_64")
DEFAULT_ABI="arm64-v8a"

RUST_FEATURES=""
GRADLE_FLAVOR="noAiDebug"
APK_VARIANT="noAiDebug"
REQUESTED_ABI=""

usage() {
    echo "用法: $0 [--ai|--no-ai] [--abi <abi>]"
    echo ""
    echo "参数:"
    echo "  --ai       启用 AI 构建变体"
    echo "  --no-ai    禁用 AI 功能（默认）"
    echo "  --abi <abi>  目标 ABI: arm64-v8a, x86_64, universal"
    echo "               默认: arm64-v8a"
    echo "               universal = arm64-v8a + x86_64"
    echo ""
    echo "示例:"
    echo "  $0 --no-ai --abi arm64-v8a"
    echo "  $0 --no-ai --abi x86_64"
    echo "  $0 --no-ai --abi universal"
    echo "  $0 --ai --abi arm64-v8a"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --ai)
            RUST_FEATURES="ai"
            GRADLE_FLAVOR="aiDebug"
            APK_VARIANT="aiDebug"
            shift
            ;;
        --no-ai)
            RUST_FEATURES=""
            GRADLE_FLAVOR="noAiDebug"
            APK_VARIANT="noAiDebug"
            shift
            ;;
        --abi)
            REQUESTED_ABI="$2"
            shift 2
            ;;
        *)
            echo "未知参数: $1"
            usage
            ;;
    esac
done

if [ -z "$REQUESTED_ABI" ]; then
    REQUESTED_ABI="$DEFAULT_ABI"
fi

ABI_LIST=""
case "$REQUESTED_ABI" in
    arm64-v8a)
        ABI_LIST="arm64-v8a"
        ;;
    x86_64)
        ABI_LIST="x86_64"
        ;;
    universal)
        ABI_LIST="arm64-v8a,x86_64"
        ;;
    *)
        echo "错误: 不支持的 ABI '$REQUESTED_ABI'，只允许: arm64-v8a, x86_64, universal"
        exit 1
        ;;
esac

VARIANT_NAME=""
if [ -n "$RUST_FEATURES" ]; then
    VARIANT_NAME="aiDebug"
else
    VARIANT_NAME="noAiDebug"
fi

GENERATED_DIR="$WORKSPACE_ROOT/apps/android/app/build/generated/writer-native/$VARIANT_NAME"

echo "素笺写作 Android 构建"
echo "  变体: $VARIANT_NAME"
echo "  ABI: $ABI_LIST"
echo "  Rust features: ${RUST_FEATURES:-<无>}"
echo "  产物目录: $GENERATED_DIR"

if ! command -v cargo-ndk &> /dev/null; then
    echo "错误: cargo-ndk 未安装。请运行: cargo install cargo-ndk"
    exit 1
fi

echo ""
echo "=== 步骤 1: 构建 Rust 原生库 ==="
BUILD_NATIVE_ARGS=(
    --variant "$VARIANT_NAME"
    --abis "$ABI_LIST"
    --output "$GENERATED_DIR"
)
if [ -n "$RUST_FEATURES" ]; then
    BUILD_NATIVE_ARGS+=(--features "$RUST_FEATURES")
fi

"$DIR/android/build_native.sh" "${BUILD_NATIVE_ARGS[@]}"

echo ""
echo "=== 步骤 2: 生成 UniFFI Kotlin 绑定 ==="
UNIFFI_SO_ABI=""
IFS=',' read -ra ABI_ARRAY <<< "$ABI_LIST"
for abi in "${ABI_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    if [ "$abi_trimmed" = "arm64-v8a" ]; then
        UNIFFI_SO_ABI="$abi_trimmed"
        break
    fi
done
if [ -z "$UNIFFI_SO_ABI" ]; then
    UNIFFI_SO_ABI="${ABI_ARRAY[0]}"
    UNIFFI_SO_ABI=$(echo "$UNIFFI_SO_ABI" | xargs)
fi

UNIFFI_SO_PATH="$GENERATED_DIR/$UNIFFI_SO_ABI/libuniffi_writer_core.so"
if [ ! -f "$UNIFFI_SO_PATH" ]; then
    echo "错误: UniFFI 绑定所需的 .so 文件不存在: $UNIFFI_SO_PATH"
    exit 1
fi

UNIFFI_OUT_DIR="$WORKSPACE_ROOT/apps/android/app/build/generated/writer-uniffi/$VARIANT_NAME/kotlin"
mkdir -p "$UNIFFI_OUT_DIR"
rm -rf "$UNIFFI_OUT_DIR/uniffi"

cd "$WORKSPACE_ROOT"
cargo run --bin uniffi-bindgen -p writer_uniffi -- generate \
    --library "$UNIFFI_SO_PATH" \
    --language kotlin \
    --out-dir "$UNIFFI_OUT_DIR"

echo "UniFFI Kotlin 绑定生成成功。"

echo ""
echo "=== 步骤 3: 构建 Android APK ==="
cd "$WORKSPACE_ROOT/apps/android"

GRADLE_ABI_PROP="$ABI_LIST"

echo "Gradle 构建: assemble${GRADLE_FLAVOR^}"
echo "  ABI property: $GRADLE_ABI_PROP"
echo "  Native dir: $GENERATED_DIR"

./gradlew "assemble${GRADLE_FLAVOR^}" \
    -Psujian.android.abis="$GRADLE_ABI_PROP" \
    -Psujian.android.nativeDir="$GENERATED_DIR"

echo ""
echo "=== 步骤 4: 验证 APK ==="
APK_DIR="app/build/outputs/apk/${APK_VARIANT%Debug}/debug"
APK_PATH=$(find "$APK_DIR" -name "sujian-android-*.apk" -type f 2>/dev/null | head -1)

if [ -z "$APK_PATH" ]; then
    APK_PATH="app/build/outputs/apk/${APK_VARIANT%Debug}/debug/app-${APK_VARIANT}.apk"
fi

if [ ! -f "$APK_PATH" ]; then
    echo "警告: 未找到 APK 文件，跳过验证"
else
    echo "APK 文件: apps/android/$APK_PATH"
    "$DIR/android/verify_apk_abis.sh" "$APK_PATH" "$ABI_LIST"
fi

echo ""
echo "构建完成 ✓"
