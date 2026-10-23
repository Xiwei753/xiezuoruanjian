#!/bin/bash
#
# fetch-model.sh — 构建期内置 MacBERT4CSC ONNX 模型与平台原生 ONNX Runtime 获取
# （Issue #985 架构换代）。
#
# 模型与运行时都不入库。脚本调用 crates/sujian-proof/tools/fetch_model.py，
# 按 crates/sujian-proof/assets/model_manifest.json 锁定的版本、字节数和
# SHA-256 校验。首次下载后文件缓存在
# ${XDG_CACHE_HOME:-$HOME/.cache}/sujian/models（SUJIAN_MODEL_CACHE 可覆盖），
# 输出目录只做硬链接/复制；后续构建和不同输出目录不会重新下载 474MB 权重。
#
# 用法:
#   packaging/correction/fetch-model.sh <输出目录>
#
# 输出:
#   <输出目录>/macbert4csc/{model.onnx,vocab.txt,...}
#   <输出目录>/runtime/libonnxruntime.so*（SUJIAN_RUNTIME_PLATFORM 指定平台，
#   或 SUJIAN_FETCH_ORT=1 时取 linux-x64）
#
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "用法: $0 <输出目录>" >&2
    exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUTPUT_DIR="$1"

if ! command -v python3 >/dev/null 2>&1; then
    echo "错误: 缺少 python3，无法获取并校验内置模型。" >&2
    exit 1
fi

mkdir -p "${OUTPUT_DIR}"
PY_ARGS=(--output "${OUTPUT_DIR}/macbert4csc")
RUNTIME_PLATFORM="${SUJIAN_RUNTIME_PLATFORM:-}"
if [ -n "${RUNTIME_PLATFORM}" ]; then
    PY_ARGS+=(--runtime "${RUNTIME_PLATFORM}" --runtime-output "${OUTPUT_DIR}/runtime")
elif [ "${SUJIAN_FETCH_ORT:-0}" = "1" ]; then
    PY_ARGS+=(--runtime linux-x64 --runtime-output "${OUTPUT_DIR}/runtime")
fi
python3 "${REPO_ROOT}/crates/sujian-proof/tools/fetch_model.py" "${PY_ARGS[@]}"
