#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "用法：$0 <HarmonyOS Native SDK 目录> <arm64 prebuilt 目录>" >&2
    exit 2
fi

NATIVE_DIR="$1"
PREBUILT_DIR="$2"
CXX_SHARED_SRC="$NATIVE_DIR/llvm/lib/aarch64-linux-ohos/c++/libc++_shared.so"
CXX_SHARED_DST="$PREBUILT_DIR/libc++_shared.so"

if [ ! -f "$CXX_SHARED_SRC" ]; then
    echo "错误：当前 HarmonyOS Native SDK 缺少 arm64 libc++_shared.so：$CXX_SHARED_SRC" >&2
    exit 1
fi

mkdir -p "$PREBUILT_DIR"
cp -- "$CXX_SHARED_SRC" "$CXX_SHARED_DST"
if ! cmp -s "$CXX_SHARED_SRC" "$CXX_SHARED_DST"; then
    echo "错误：libc++_shared.so 未能从 SDK 完整复制到 prebuilt 目录。" >&2
    exit 1
fi

echo "HarmonyOS arm64 libc++_shared.so: $CXX_SHARED_SRC -> $CXX_SHARED_DST"
