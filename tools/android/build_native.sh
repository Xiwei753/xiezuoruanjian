#!/bin/bash
set -euo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/../.." && pwd )"

VALID_ABIS=("arm64-v8a" "x86_64")
ABI_TO_TARGET=(
    "arm64-v8a:aarch64-linux-android"
    "x86_64:x86_64-linux-android"
)

VARIANT=""
ABIS=""
OUTPUT_DIR=""
RUST_FEATURES=""

usage() {
    echo "用法: $0 --variant <variant> --abis <abi1,abi2> --output <dir> [--features <features>]"
    echo ""
    echo "参数:"
    echo "  --variant <variant>   构建变体名称 (如 noAiDebug, aiDebug)"
    echo "  --abis <abi_list>     逗号分隔的 ABI 列表 (arm64-v8a, x86_64)"
    echo "  --output <dir>        产物输出根目录"
    echo "  --features <features> Rust feature 列表 (如 ai)"
    echo ""
    echo "示例:"
    echo "  $0 --variant noAiDebug --abis arm64-v8a,x86_64 --output apps/android/app/build/generated/writer-native/noAiDebug"
    echo "  $0 --variant aiDebug --abis arm64-v8a --output apps/android/app/build/generated/writer-native/aiDebug --features ai"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --variant)
            VARIANT="$2"
            shift 2
            ;;
        --abis)
            ABIS="$2"
            shift 2
            ;;
        --output)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --features)
            RUST_FEATURES="$2"
            shift 2
            ;;
        *)
            echo "未知参数: $1"
            usage
            ;;
    esac
done

if [ -z "$VARIANT" ] || [ -z "$ABIS" ] || [ -z "$OUTPUT_DIR" ]; then
    echo "错误: --variant, --abis, --output 为必填参数"
    usage
fi

IFS=',' read -ra ABI_ARRAY <<< "$ABIS"

for abi in "${ABI_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    valid=false
    for valid_abi in "${VALID_ABIS[@]}"; do
        if [ "$abi_trimmed" = "$valid_abi" ]; then
            valid=true
            break
        fi
    done
    if [ "$valid" = false ]; then
        echo "错误: 不支持的 ABI '$abi_trimmed'，只允许: ${VALID_ABIS[*]}"
        exit 1
    fi
done

if ! command -v cargo-ndk &> /dev/null; then
    echo "错误: cargo-ndk 未安装。请运行: cargo install cargo-ndk"
    exit 1
fi

for abi in "${ABI_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    target=""
    for mapping in "${ABI_TO_TARGET[@]}"; do
        map_abi="${mapping%%:*}"
        map_target="${mapping##*:}"
        if [ "$abi_trimmed" = "$map_abi" ]; then
            target="$map_target"
            break
        fi
    done
    if [ -n "$target" ]; then
        if ! rustup target list --installed 2>/dev/null | grep -q "$target"; then
            echo "错误: Rust target $target 未安装。请运行: rustup target add $target"
            exit 1
        fi
    fi
done

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    if [ -n "${ANDROID_NDK_ROOT:-}" ]; then
        ANDROID_NDK_HOME="$ANDROID_NDK_ROOT"
    else
        echo "错误: ANDROID_NDK_HOME 未设置"
        exit 1
    fi
fi

echo "构建 Rust 原生库:"
echo "  变体: $VARIANT"
echo "  ABI: $ABIS"
echo "  输出: $OUTPUT_DIR"
echo "  Rust features: ${RUST_FEATURES:-<无>}"

for abi in "${ABI_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    abi_dir="$OUTPUT_DIR/$abi_trimmed"
    echo "清理旧产物: $abi_dir/libuniffi_writer_core.so"
    rm -f "$abi_dir/libuniffi_writer_core.so"
done

CARGO_NDK_TARGETS=()
for abi in "${ABI_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    CARGO_NDK_TARGETS+=("-t" "$abi_trimmed")
done

FEATURE_ARGS=()
if [ -n "$RUST_FEATURES" ]; then
    FEATURE_ARGS+=(--features "$RUST_FEATURES")
fi

echo "编译 Rust 原生库 (cargo-ndk)..."
cd "$WORKSPACE_ROOT/platform/rust/android"

cargo ndk "${CARGO_NDK_TARGETS[@]}" -o "$OUTPUT_DIR" build --release "${FEATURE_ARGS[@]}"

echo "验证产物..."
for abi in "${ABI_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    so_path="$OUTPUT_DIR/$abi_trimmed/libuniffi_writer_core.so"

    if [ ! -f "$so_path" ]; then
        echo "错误: $abi_trimmed 的 libuniffi_writer_core.so 未生成"
        exit 1
    fi

    echo "  $abi_trimmed: $so_path ✓"
done

echo "Rust 原生库构建完成。"
