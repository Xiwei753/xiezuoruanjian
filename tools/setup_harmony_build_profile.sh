#!/bin/bash
# =============================================================================
# HarmonyOS build-profile 生成脚本
# =============================================================================
#
# 仓库只跟踪公共模板 apps/harmony/build-profile.template.json5（空签名
# material）；真正参与构建的 apps/harmony/build-profile.json5 不入库，是本机/CI
# 的工作文件，会被 devecocli signature generate / DevEco Studio 写入本机签名
# 信息，不要提交。
#
# 本脚本在目标文件缺失时从模板复制生成；已存在则原样保留并提示退出，绝不覆盖
# 本机已填好的签名 material。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$REPO_ROOT/apps/harmony/build-profile.template.json5"
TARGET="$REPO_ROOT/apps/harmony/build-profile.json5"

if [[ ! -f "$TEMPLATE" ]]; then
    echo "缺少公共模板：$TEMPLATE" >&2
    exit 1
fi

if [[ -f "$TARGET" ]]; then
    echo "保留已有 $TARGET（本机/CI 生成文件，不入库）。"
    exit 0
fi

cp "$TEMPLATE" "$TARGET"
echo "已从模板生成 $TARGET。"
echo "本机签名：运行 devecocli signature generate（或 DevEco Studio）写入 signingConfig material。"
