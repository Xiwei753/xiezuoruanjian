#!/bin/bash
#
# fetch-model.sh — 构建期内置中文 KenLM 模型获取与校验（Issue #958）
#
# 把 kenlm-model.lock 锁定的 people2014corpus_chars.klm 放到指定输出目录。
# RPM / APK / HAP 三个平台的构建脚本都调用本脚本；模型只作为构建产物，
# 不入库、不随应用运行时下载：
#   - 输出目录已有摘要正确、字节数一致的文件时直接复用；
#   - 否则从锁定 URL 下载到临时文件，校验字节数与 SHA-256；
#   - 校验通过后原子改名为 people2014corpus_chars.klm。
# 下载失败或校验不符时以非零状态退出，让构建失败，而不是打出缺少内置
# 模型或模型内容不符的安装包。
#
# 用法:
#   packaging/correction/fetch-model.sh <输出目录>
#
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "用法: $0 <输出目录>" >&2
    exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCK_FILE="${SCRIPT_DIR}/kenlm-model.lock"
OUTPUT_DIR="$1"

if [ ! -f "${LOCK_FILE}" ]; then
    echo "错误: 找不到模型锁定文件 ${LOCK_FILE}" >&2
    exit 1
fi

read_lock_field() {
    local field="$1"
    local value
    value="$(sed -n "s/^${field}=//p" "${LOCK_FILE}" | head -n 1)"
    if [ -z "${value}" ]; then
        echo "错误: 锁定文件 ${LOCK_FILE} 缺少字段 ${field}" >&2
        exit 1
    fi
    printf '%s' "${value}"
}

FILENAME="$(read_lock_field filename)"
SOURCE="$(read_lock_field source)"
EXPECTED_SIZE="$(read_lock_field size)"
EXPECTED_SHA256="$(read_lock_field sha256)"

for tool in curl sha256sum; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "错误: 缺少命令 ${tool}，无法获取并校验内置模型。" >&2
        exit 1
    fi
done

TARGET="${OUTPUT_DIR}/${FILENAME}"
mkdir -p "${OUTPUT_DIR}"

file_matches_lock() {
    local path="$1"
    [ -f "${path}" ] || return 1
    [ "$(wc -c < "${path}" | tr -d ' ')" = "${EXPECTED_SIZE}" ] || return 1
    [ "$(sha256sum "${path}" | cut -d' ' -f1)" = "${EXPECTED_SHA256}" ] || return 1
    return 0
}

if file_matches_lock "${TARGET}"; then
    echo "==> 复用已校验的内置模型: ${TARGET}"
    exit 0
fi

TEMP_FILE="${OUTPUT_DIR}/.${FILENAME}.download.$$"
cleanup() {
    rm -f "${TEMP_FILE}"
}
trap cleanup EXIT

echo "==> 下载内置模型: ${SOURCE}"
if ! curl -fL --retry 3 --retry-delay 5 -o "${TEMP_FILE}" "${SOURCE}"; then
    echo "错误: 内置模型下载失败，构建中止。" >&2
    exit 1
fi

ACTUAL_SIZE="$(wc -c < "${TEMP_FILE}" | tr -d ' ')"
if [ "${ACTUAL_SIZE}" != "${EXPECTED_SIZE}" ]; then
    echo "错误: 内置模型字节数不符（期望 ${EXPECTED_SIZE}，实际 ${ACTUAL_SIZE}），构建中止。" >&2
    exit 1
fi

ACTUAL_SHA256="$(sha256sum "${TEMP_FILE}" | cut -d' ' -f1)"
if [ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]; then
    echo "错误: 内置模型 SHA-256 不符（期望 ${EXPECTED_SHA256}，实际 ${ACTUAL_SHA256}），构建中止。" >&2
    exit 1
fi

mv "${TEMP_FILE}" "${TARGET}"
trap - EXIT
echo "==> 已校验并写入内置模型: ${TARGET}"
