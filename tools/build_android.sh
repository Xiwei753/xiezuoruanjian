#!/bin/bash
# =============================================================================
# Android 构建脚本
# =============================================================================
#
# 本脚本用于构建 Android 应用的完整流程，包括：
# 1. 编译 Rust JNI 库（使用 cargo-ndk）
# 2. 构建 Android APK（使用 Gradle）
# 3. 验证 JNI 库是否正确打包到 APK 中
#
# 使用方法：
#   ./build_android.sh [--ai|--no-ai]
#
# 参数说明：
#   --ai    : 预留 AI 构建变体，当前功能未开放
#   --no-ai : 禁用 AI 功能（默认选项）
#
# 环境要求：
#   - Rust 工具链（rustup, cargo）
#   - cargo-ndk（可通过 cargo install cargo-ndk 安装）
#   - Android NDK（需要设置 ANDROID_NDK_HOME 环境变量）
#   - Android SDK
#   - Java JDK
#
# 构建产物：
#   - Rust JNI 库: apps/android/app/src/main/jniLibs/arm64-v8a/libwriter_core_jni.so
#   - Android APK: apps/android/app/build/outputs/apk/*/debug/sujian-android-*.apk
#
# 注意事项：
#   - 本脚本只支持 arm64-v8a 架构（官方只支持 64 位 ARM 设备）
#   - 构建过程中会自动清理旧的 JNI 库文件
#   - 如果构建失败，请检查环境变量和依赖是否正确安装

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "$DIR/.." && pwd )"

# 解析命令行参数
RUST_FEATURES=""
GRADLE_FLAVOR="noAiDebug"
APK_VARIANT="noAiDebug"

for arg in "$@"; do
    case $arg in
        --ai)
            RUST_FEATURES="--features ai"
            GRADLE_FLAVOR="aiDebug"
            APK_VARIANT="aiDebug"
            ;;
        --no-ai)
            RUST_FEATURES=""
            GRADLE_FLAVOR="noAiDebug"
            APK_VARIANT="noAiDebug"
            ;;
        *)
            echo "未知参数: $arg"
            echo "使用方法: $0 [--ai|--no-ai]"
            exit 1
            ;;
    esac
done

echo "开始构建素笺写作 Android JNI 库..."
echo "  Rust 特性: ${RUST_FEATURES:-<无>}"
echo "  Gradle 构建类型: $GRADLE_FLAVOR"

# 检查 cargo-ndk 是否已安装
if ! command -v cargo-ndk &> /dev/null; then
    echo "错误: cargo-ndk 未安装。"
    echo "要构建 Rust JNI 库，请通过以下命令安装: cargo install cargo-ndk"
    echo "确保 ANDROID_NDK_HOME 环境变量已正确设置。"
    echo ""
    echo "中止构建以防止缺少 libwriter_core_jni.so 文件。"
    exit 1
fi

echo "使用 cargo-ndk 编译 arm64-v8a 架构..."
cd "$WORKSPACE_ROOT/bindings/android"

# shellcheck disable=SC2086
cargo ndk -t arm64-v8a -o "$WORKSPACE_ROOT/apps/android/app/src/main/jniLibs" build --release $RUST_FEATURES

if [ $? -eq 0 ]; then
    echo "Rust JNI 库构建成功并已复制到 jniLibs 目录。"
else
    echo "Rust JNI 库构建失败。"
    exit 1
fi

cd "$WORKSPACE_ROOT/apps/android"

echo "开始构建 Android 应用 ($GRADLE_FLAVOR)..."
./gradlew "assemble${GRADLE_FLAVOR^}"

if [ $? -eq 0 ]; then
    echo "Android 应用构建成功。"

    # 查找生成的 APK 文件
    APK_DIR="app/build/outputs/apk/${APK_VARIANT%Debug}/debug"
    APK_PATH=$(find "$APK_DIR" -name "sujian-android-*.apk" -type f 2>/dev/null | head -1)

    if [ -z "$APK_PATH" ]; then
        # 如果找不到自定义名称的 APK，使用默认名称
        APK_PATH="app/build/outputs/apk/${APK_VARIANT%Debug}/debug/app-${APK_VARIANT}.apk"
    fi

    echo "APK 文件位置: apps/android/$APK_PATH"

    # 验证 JNI 库是否正确打包到 APK 中
    echo "验证 APK 中的 JNI .so 文件..."
    if unzip -l "$APK_PATH" | grep -q "lib/arm64-v8a/libwriter_core_jni.so"; then
        echo "验证通过: 在 arm64-v8a 目录中找到 libwriter_core_jni.so"
    else
        echo "错误: 在 APK 的 arm64-v8a 目录中未找到 libwriter_core_jni.so！"
        exit 1
    fi
else
    echo "Android 应用构建失败。"
    exit 1
fi
