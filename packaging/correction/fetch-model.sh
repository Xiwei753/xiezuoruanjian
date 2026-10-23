#!/bin/bash
#
# fetch-model.sh — 构建期内置 MacBERT4CSC ONNX 模型与 Linux ONNX Runtime 获取
# （Issue #985 架构换代）。
#
# 模型与运行时都不入库。脚本调用 crates/sujian-proof/tools/fetch_model.py，
# 按 crates/sujian-proof/assets/model_manifest.json 锁定的版本、字节数和
# SHA-256 下载校验；任何不符都会让构建失败，而不是打出缺模型或错模型的包。
#
# 用法:
#   packaging/correction/fetch-model.sh <输出目录>
#
# 输出:
#   <输出目录>/macbert4csc/{model.onnx,vocab.txt,...}
#   <输出目录>/runtime/libonnxruntime.so.1.22.0（仅 SUJIAN_FETCH_ORT=1）
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
if [ "${SUJIAN_FETCH_ORT:-0}" = "1" ]; then
    PY_ARGS+=(--runtime linux-x64 --runtime-output "${OUTPUT_DIR}/runtime")
fi
python3 "${REPO_ROOT}/crates/sujian-proof/tools/fetch_model.py" "${PY_ARGS[@]}"
