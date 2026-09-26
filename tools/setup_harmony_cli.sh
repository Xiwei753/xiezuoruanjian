#!/bin/bash
# =============================================================================
# HarmonyOS CLI 工具链安装脚本
# =============================================================================
#
# 安装 HarmonyOS 命令行工具（ohpm、codelinter、hvigorw）并加入 PATH。
# 固定版本，供 CI workflow 和本地开发统一使用。
#
# 使用方法：
#   source tools/setup_harmony_cli.sh
#
# 安装后以下命令可直接使用：
#   ohpm — HarmonyOS 包管理器
#   codelinter — ArkTS 代码静态检测
#   hvigorw — HarmonyOS 构建工具
#
# 环境变量：
#   HARMONY_CLI_VERSION — 固定版本号（默认: 5.0.5）
#   HARMONY_CLI_HOME — 安装目录（默认: $HOME/.harmony-cli）

set -euo pipefail

HARMONY_CLI_VERSION="${HARMONY_CLI_VERSION:-5.0.5}"
HARMONY_CLI_HOME="${HARMONY_CLI_HOME:-$HOME/.harmony-cli}"

# 检查是否已安装且版本匹配
if [ -x "$HARMONY_CLI_HOME/bin/ohpm" ] && [ -f "$HARMONY_CLI_HOME/VERSION" ] && [ "$(cat "$HARMONY_CLI_HOME/VERSION")" = "$HARMONY_CLI_VERSION" ]; then
  echo "HarmonyOS CLI $HARMONY_CLI_VERSION 已安装，跳过安装。"
  export PATH="$HARMONY_CLI_HOME/bin:$PATH"
  return 0 2>/dev/null || exit 0
fi

echo "=== 安装 HarmonyOS CLI $HARMONY_CLI_VERSION ==="

# 创建安装目录
mkdir -p "$HARMONY_CLI_HOME/bin"
mkdir -p "$HARMONY_CLI_HOME/lib"

# 下载并安装 command-line-tools
# 官方下载地址：https://developer.huawei.com/consumer/cn/download/
# CLI 包名格式：command-line-tools-linux-x64-{version}.zip
CLI_URL="https://contentcenter-vali-drcn.dbankcdn.cn/pvt_2/DeveloperAlliance_package_901_9/81/v3/00BjWG6lRNOlKs2xQ3WJfQ/commandlinetools-linux-x64-${HARMONY_CLI_VERSION}.zip"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "下载 CLI 包..."
if ! curl -fSL -o "$TMP_DIR/cli.zip" "$CLI_URL"; then
  echo "错误：下载 HarmonyOS CLI 失败。请检查版本号和网络连接。" >&2
  echo "可手动下载并设置 HARMONY_CLI_HOME 指向解压目录。" >&2
  exit 1
fi

echo "解压 CLI 包..."
if ! unzip -q "$TMP_DIR/cli.zip" -d "$TMP_DIR/cli"; then
  echo "错误：解压 HarmonyOS CLI 失败。" >&2
  exit 1
fi

# 查找解压后的工具目录
CLI_EXTRACTED=$(find "$TMP_DIR/cli" -maxdepth 1 -type d | tail -1)
if [ ! -d "$CLI_EXTRACTED" ]; then
  echo "错误：未找到解压后的 CLI 目录。" >&2
  exit 1
fi

# 复制工具到安装目录
cp -r "$CLI_EXTRACTED/bin/"* "$HARMONY_CLI_HOME/bin/"
cp -r "$CLI_EXTRACTED/lib/"* "$HARMONY_CLI_HOME/lib/" 2>/dev/null || true

# 标记版本
echo "$HARMONY_CLI_VERSION" > "$HARMONY_CLI_HOME/VERSION"

# 加入 PATH
export PATH="$HARMONY_CLI_HOME/bin:$PATH"

echo ""
echo "=== HarmonyOS CLI 安装完成 ==="
echo "  版本: $HARMONY_CLI_VERSION"
echo "  安装目录: $HARMONY_CLI_HOME"
echo "  PATH 已更新"
echo ""
echo "可用命令: ohpm, codelinter, hvigorw"
