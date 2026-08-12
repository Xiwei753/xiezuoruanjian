#!/bin/bash
set -euo pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"

VALID_ABIS=("arm64-v8a" "x86_64")
DEFAULT_ABI="arm64-v8a"

FLAVOR_NAME="noAi"
REQUESTED_ABI=""
SKIP_GRADLE=false

usage() {
    echo "用法: $0 [--ai|--no-ai] [--abi <abi>] [--skip-gradle]"
    echo ""
    echo "参数:"
    echo "  --ai       启用 AI 构建变体"
    echo "  --no-ai    禁用 AI 功能（默认）"
    echo "  --abi <abi>  目标 ABI: arm64-v8a, x86_64, universal"
    echo "               默认: arm64-v8a"
    echo "               universal = arm64-v8a + x86_64"
    echo "  --skip-gradle  只生成原生库和 UniFFI 绑定，跳过 APK 构建"
    echo ""
    echo "示例:"
    echo "  $0 --no-ai --abi arm64-v8a"
    echo "  $0 --no-ai --abi x86_64"
    echo "  $0 --no-ai --abi universal"
    echo "  $0 --ai --abi arm64-v8a"
    echo "  $0 --no-ai --abi x86_64 --skip-gradle"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --ai)
            FLAVOR_NAME="ai"
            shift
            ;;
        --no-ai)
            FLAVOR_NAME="noAi"
            shift
            ;;
        --abi)
            REQUESTED_ABI="$2"
            shift 2
            ;;
        --skip-gradle)
            SKIP_GRADLE=true
            shift
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

if [ "$FLAVOR_NAME" = "ai" ]; then
    VARIANT_NAME="aiDebug"
else
    VARIANT_NAME="noAiDebug"
fi

echo "素笺写作 Android 构建"
echo "  变体: $VARIANT_NAME"
echo "  Flavor: $FLAVOR_NAME"
echo "  ABI: $ABI_LIST"

# #618 二：原生库与 UniFFI 绑定只剩 Gradle 一份依赖图（build<Variant>WriterNative +
# generate<Variant>WriterUniffi），本脚本不再自己维护"先 native、再 bindgen、再 Gradle"
# 的增量逻辑，只解析 flavor/ABI 参数并调用 Gradle。
cd "$WORKSPACE_ROOT/apps/android"

VARIANT_CAPITALIZED="$(echo "$VARIANT_NAME" | cut -c1 | tr '[:lower:]' '[:upper:]')$(echo "$VARIANT_NAME" | cut -c2-)"

if [ "$SKIP_GRADLE" = true ]; then
    GRADLE_TASK="generate${VARIANT_CAPITALIZED}WriterUniffi"
    echo "Gradle 任务: $GRADLE_TASK（原生库 + UniFFI 绑定，跳过 APK）"
    ./gradlew "$GRADLE_TASK" -Psujian.android.abis="$ABI_LIST"
    echo "原生库与 UniFFI 绑定生成成功。"
    exit 0
fi

GRADLE_TASK="assemble${VARIANT_CAPITALIZED}"
echo "Gradle 构建: $GRADLE_TASK"
echo "  ABI property: $ABI_LIST"

./gradlew "$GRADLE_TASK" -Psujian.android.abis="$ABI_LIST"

echo ""
echo "=== 步骤 4: 验证 APK ==="
APK_DIR="app/build/outputs/apk/$FLAVOR_NAME/debug"
# 注意 set -o pipefail：ls 无匹配会以非零退出，必须 || true 才能走到下面的 fallback。
APK_PATH=$(ls -t "$APK_DIR"/sujian-android-*.apk 2>/dev/null | head -1 || true)
if [ -z "$APK_PATH" ]; then
    APK_PATH=$(ls -t "$APK_DIR"/app-*.apk 2>/dev/null | head -1 || true)
fi

if [ -z "$APK_PATH" ] || [ ! -f "$APK_PATH" ]; then
    echo "错误: 未找到 APK 文件，无法进行 ABI 验证: $APK_DIR"
    exit 1
fi

echo "APK 文件: apps/android/$APK_PATH"
"$DIR/android/verify_apk_abis.sh" "$APK_PATH" "$ABI_LIST"

echo ""
echo "构建完成 ✓"
