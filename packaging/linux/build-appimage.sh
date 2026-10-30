#!/bin/bash
# =============================================================================
# Build a self-contained Linux AppImage for Issue #985.
#
# The RPM path installs the ONNX model and Runtime under /usr/share and
# /usr/lib. AppImage has no such system prefix, so this script stages the same
# resources inside the AppDir and lets the launcher export:
#   SUJIAN_CORRECTION_MODEL_DIR=$APPDIR/usr/share/sujian/models/macbert4csc
#   SUJIAN_ONNXRUNTIME_LIB=$APPDIR/usr/lib/sujian/libonnxruntime.so
#
# Requires: cargo, linuxdeploy with the Qt plugin, appimagetool, python3.
# Override tool paths with LINUXDEPLOY / APPIMAGETOOL / QMAKE.
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"
APPDIR="$OUT_DIR/AppDir"
BUNDLE_DIR="$OUT_DIR/correction-bundle"
BINARY="$ROOT/target/release/sujian-linux-qt"
MCP_BINARY="$ROOT/target/release/sujian_mcp"
# 侧载 Agent 的只读作品视图依赖 bubblewrap；AppImage 不能要求系统包管理器
# 自动安装它，因此把构建机的 bwrap 一并放进 AppDir（系统已有 bwrap 时优先用系统版本）。
BWRAP="${BWRAP:-$(command -v bwrap || true)}"
LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy}"
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"
# QML is embedded in the Rust binary via qrc, so linuxdeploy cannot discover
# QtQuick/Kirigami imports from ELF dependencies alone.
QML_SOURCES_PATHS="$ROOT/apps/Linux_qt/qml"
export QML_SOURCES_PATHS

for tool in "$LINUXDEPLOY" "$APPIMAGETOOL"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "错误：找不到 $tool；请安装 linuxdeploy/appimagetool，或用环境变量指定路径。" >&2
        exit 1
    fi
done
if [ -z "$BWRAP" ] || [ ! -x "$BWRAP" ]; then
    echo "错误：AppImage 需要 bubblewrap；请安装 bubblewrap 或用 BWRAP=/path/to/bwrap 指定。" >&2
    exit 1
fi

echo "==> 构建 Linux Qt release 与 MCP Server"
# Issue #1014：MCP 与 Qt 客户端同次构建，AppImage 内置稳定入口。
cargo build --release -p sujian-linux-qt -p sujian_mcp --manifest-path "$ROOT/Cargo.toml"

echo "==> 获取并校验模型与 ONNX Runtime（持久缓存复用）"
rm -rf "$BUNDLE_DIR"
SUJIAN_FETCH_ORT=1 bash "$ROOT/packaging/correction/fetch-model.sh" "$BUNDLE_DIR"

rm -rf "$APPDIR"
mkdir -p \
    "$APPDIR/usr/bin" \
    "$APPDIR/usr/libexec/sujian" \
    "$APPDIR/usr/lib/sujian" \
    "$APPDIR/usr/share/sujian/models/macbert4csc" \
    "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps"

cp "$BINARY" "$APPDIR/usr/bin/sujian-linux-qt"
# Issue #1014：MCP 放在内部固定路径，AppRun 的 --sujian-mcp 入口转发到这里；
# 不把 /tmp/.mount_* 下的临时路径交给外部 CLI。
cp "$MCP_BINARY" "$APPDIR/usr/libexec/sujian/sujian_mcp"
# AppImage 内置 bwrap；系统有 bwrap 时 AppRun 仍优先用系统版本（可能是
# setuid 安装），没有时回退到这里的副本。
cp "$BWRAP" "$APPDIR/usr/libexec/sujian/bwrap"
cp "$ROOT/packaging/linux/sujian-launcher.sh" "$APPDIR/usr/libexec/sujian/sujian-launcher.sh"
chmod +x "$APPDIR/usr/bin/sujian-linux-qt" "$APPDIR/usr/libexec/sujian/sujian_mcp" \
    "$APPDIR/usr/libexec/sujian/bwrap" "$APPDIR/usr/libexec/sujian/sujian-launcher.sh"
ln -sf ../libexec/sujian/sujian-launcher.sh "$APPDIR/usr/bin/sujian"
cp "$ROOT/packaging/linux/sujian.desktop" "$APPDIR/usr/share/applications/sujian.desktop"
cp "$ROOT/packaging/linux/icons/hicolor/256x256/apps/sujian.png" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps/sujian.png"
cp "$ROOT/packaging/linux/icons/hicolor/scalable/apps/sujian.svg" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps/sujian.svg"

# The fetch script materializes model files and the versioned Runtime library.
cp -a "$BUNDLE_DIR/macbert4csc/." "$APPDIR/usr/share/sujian/models/macbert4csc/"
cp "$BUNDLE_DIR/runtime/libonnxruntime.so."* "$APPDIR/usr/lib/sujian/libonnxruntime.so"

# Issue #1014：MCP 自身 AGPL 许可证随 AppImage 提供。
mkdir -p "$APPDIR/usr/share/licenses/sujian/sujian_mcp"
cp "$ROOT/apps/sujian_mcp/LICENSE" "$APPDIR/usr/share/licenses/sujian/sujian_mcp/LICENSE"

echo "==> 部署 Qt 运行库/插件"
# MCP 是独立 Rust ELF；把它也交给 linuxdeploy 处理动态库。
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/sujian-linux-qt" \
    --executable "$APPDIR/usr/libexec/sujian/sujian_mcp" \
    --executable "$APPDIR/usr/libexec/sujian/bwrap" \
    --desktop-file "$APPDIR/usr/share/applications/sujian.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/sujian.png" \
    --plugin qt

# linuxdeploy may generate its own AppRun. Install the reviewed entry script,
# which sources linuxdeploy's apprun-hooks (including the Qt hook) and then
# executes the same launcher used by the RPM.
install -m 755 "$ROOT/packaging/linux/AppRun-appimage.sh" "$APPDIR/AppRun"

echo "==> 生成 AppImage"
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$OUT_DIR/素笺写作-x86_64.AppImage"
echo "完成：$OUT_DIR/素笺写作-x86_64.AppImage"
