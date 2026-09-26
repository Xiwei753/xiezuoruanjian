#!/bin/bash
# =============================================================================
# HarmonyOS CLI 工具链安装脚本
# =============================================================================
#
# 安装 HarmonyOS 命令行工具（ohpm、codelinter、hvigorw、SDK、Node）并加入 PATH。
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
#   HARMONY_CLI_VERSION — 固定版本号（默认: 26.0.0.821）
#   HARMONY_CLI_HOME — 安装目录（默认: $HOME/.harmony-cli）
#
# 注：HARMONY_CLI_HOME 直接指向解压后的 command-line-tools 目录，
#     保留完整结构（bin/、lib/、tool/node/、sdk/ 等），
#     不拆目录复制，确保 hvigorw、Native 构建等都能正常工作。

set -euo pipefail

# -----------------------------------------------------------------------------
# 固定配置：版本号、下载地址、SHA256
# -----------------------------------------------------------------------------
# 版本号必须带完整 build 号（如 26.0.0.821），不要截断。
# 此版本与 apps/harmony/build-profile.json5 的 targetSdkVersion 26.0.0 配套。
# 版本对照：26.0.0.821 = API 26 正式版（HarmonyOS 6.0）
HARMONY_CLI_VERSION="${HARMONY_CLI_VERSION:-26.0.0.821}"
HARMONY_CLI_HOME="${HARMONY_CLI_HOME:-$HOME/.harmony-cli}"

# 下载源：ErBWs/ohos-sdk 社区镜像（华为官方下载中心链接带时效签名，无法固定）。
# 该镜像提供 GitHub Release 直链，分片存储（.aa/.ab），拼接后得到完整 tar.gz。
# 镜像地址：https://github.com/ErBWs/ohos-sdk/releases/tag/${HARMONY_CLI_VERSION}
CLI_BASE_URL="https://github.com/ErBWs/ohos-sdk/releases/download/${HARMONY_CLI_VERSION}"
CLI_FILENAME="ohos-sdk-linux-amd64.tar.gz"

# SHA256 校验值（必须固定，不允许空值）
# 来源：https://github.com/ErBWs/ohos-sdk/releases/download/26.0.0.821/ohos-sdk-linux-amd64.tar.gz.sha256
CLI_SHA256="0cbdf7ac5c1be1e42694d448ffaee0c0be0ba9a948197e2bc5b61ca5bdc481f2"

# 检查是否已安装且版本匹配
if [ -x "$HARMONY_CLI_HOME/bin/ohpm" ] && [ -f "$HARMONY_CLI_HOME/VERSION" ] && [ "$(cat "$HARMONY_CLI_HOME/VERSION")" = "$HARMONY_CLI_VERSION" ]; then
  echo "HarmonyOS CLI $HARMONY_CLI_VERSION 已安装，跳过安装。"
  export PATH="$HARMONY_CLI_HOME/bin:$PATH"
  return 0 2>/dev/null || exit 0
fi

echo "=== 安装 HarmonyOS CLI $HARMONY_CLI_VERSION ==="

# 创建安装目录
mkdir -p "$HARMONY_CLI_HOME"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

# -----------------------------------------------------------------------------
# 下载分片文件并拼接
# -----------------------------------------------------------------------------
echo "下载 CLI 包（分片文件）..."

# 下载分片 .aa
if ! curl -fSL -o "$TMP_DIR/${CLI_FILENAME}.aa" "${CLI_BASE_URL}/${CLI_FILENAME}.aa"; then
  echo "错误：下载分片 .aa 失败。" >&2
  echo "请检查版本号和网络连接。" >&2
  exit 1
fi

# 下载分片 .ab
if ! curl -fSL -o "$TMP_DIR/${CLI_FILENAME}.ab" "${CLI_BASE_URL}/${CLI_FILENAME}.ab"; then
  echo "错误：下载分片 .ab 失败。" >&2
  exit 1
fi

# 拼接分片为完整归档
cat "$TMP_DIR/${CLI_FILENAME}.aa" "$TMP_DIR/${CLI_FILENAME}.ab" > "$TMP_DIR/$CLI_FILENAME"

# SHA256 校验（强制执行，不允许跳过）
echo "校验 SHA256..."
ACTUAL_SHA256=$(sha256sum "$TMP_DIR/$CLI_FILENAME" | awk '{print $1}')
if [ "$ACTUAL_SHA256" != "$CLI_SHA256" ]; then
  echo "错误：SHA256 校验失败。" >&2
  echo "  期望: $CLI_SHA256" >&2
  echo "  实际: $ACTUAL_SHA256" >&2
  exit 1
fi
echo "SHA256 校验通过。"

# -----------------------------------------------------------------------------
# 解压并安装
# -----------------------------------------------------------------------------
echo "解压 CLI 包..."
if ! tar -xzf "$TMP_DIR/$CLI_FILENAME" -C "$TMP_DIR/cli"; then
  echo "错误：解压 HarmonyOS CLI 失败。" >&2
  exit 1
fi

# 查找解压后的 command-line-tools 目录
# 官方包解压后是完整的 command-line-tools/ 目录结构，包含：
#   bin/（ohpm、codelinter、hvigorw）
#   lib/
#   tool/node/（配套 Node.js）
#   sdk/default/openharmony/（HarmonyOS SDK）
#   sdk/default/openharmony/native/（Native SDK）
CLI_EXTRACTED=$(find "$TMP_DIR/cli" -maxdepth 1 -type d -name "command-line-tools" | head -1)
if [ -z "$CLI_EXTRACTED" ]; then
  # 如果没有 command-line-tools 子目录，取解压目录本身
  CLI_EXTRACTED=$(find "$TMP_DIR/cli" -maxdepth 1 -type d | tail -1)
fi
if [ ! -d "$CLI_EXTRACTED" ]; then
  echo "错误：未找到解压后的 CLI 目录。" >&2
  exit 1
fi

# 直接复制整个 command-line-tools 目录到 HARMONY_CLI_HOME
# 不拆目录复制，保留完整结构（SDK、Node、Native 等）
echo "安装 CLI 到 $HARMONY_CLI_HOME ..."
cp -r "$CLI_EXTRACTED/"* "$HARMONY_CLI_HOME/"
cp -r "$CLI_EXTRACTED/".[!.]* "$HARMONY_CLI_HOME/" 2>/dev/null || true

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
echo "Native SDK: $HARMONY_CLI_HOME/sdk/default/openharmony/native"
echo "内置 Node: $HARMONY_CLI_HOME/tool/node"
