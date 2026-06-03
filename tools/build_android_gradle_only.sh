#!/bin/bash
# =============================================================================
# Android Gradle 构建脚本（仅 Gradle 部分）
# =============================================================================
#
# 本脚本用于仅构建 Android 应用的 Gradle 部分，不包含 Rust native library 编译。
#
# 使用方法：
#   ./build_android_gradle_only.sh
#
# 功能说明：
#   1. 切换到 Android 应用目录
#   2. 执行 Gradle debug 模式构建
#   3. 输出构建结果和 APK 位置
#
# 环境要求：
#   - Android SDK
#   - Java JDK
#   - Gradle（项目自带 gradlew 脚本）
#
# 构建产物：
#   - Android APK: apps/android/app/build/outputs/apk/*/debug/sujian-android-*.apk
#
# 适用场景：
#   - 当 Rust native library 已经编译完成，只需要重新构建 Android 应用时
#   - 快速测试 Android 代码修改（不涉及 Rust 代码变更）
#   - 开发调试阶段的快速迭代
#
# 注意事项：
#   - 本脚本不会编译 Rust native library，需要先运行 build_android.sh 或手动编译
#   - 如果 native library 文件不存在，构建可能会失败
#   - 适用于 arm64-v8a 架构（官方只支持 64 位 ARM 设备）

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/../apps/android"

echo "开始构建 Android 应用..."
./gradlew assembleDebug
if [ $? -eq 0 ]; then
    echo "Android 应用构建成功。"
    echo "APK 文件位置: apps/android/app/build/outputs/apk/*/debug/sujian-android-*.apk"
else
    echo "Android 应用构建失败。"
    exit 1
fi
