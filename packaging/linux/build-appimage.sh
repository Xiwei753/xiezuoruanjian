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
LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy}"
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"

for tool in "$LINUXDEPLOY" "$APPIMAGETOOL"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "错误：找不到 $tool；请安装 linuxdeploy/appimagetool，或用环境变量指定路径。" >&2
        exit 1
    fi
done

echo "==> 构建 Linux Qt release"
cargo build --release -p sujian-linux-qt --manifest-path "$ROOT/Cargo.toml"

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
cp "$ROOT/packaging/linux/sujian-launcher.sh" "$APPDIR/usr/libexec/sujian/sujian-launcher.sh"
chmod +x "$APPDIR/usr/bin/sujian-linux-qt" "$APPDIR/usr/libexec/sujian/sujian-launcher.sh"
ln -sf ../libexec/sujian/sujian-launcher.sh "$APPDIR/usr/bin/sujian"
cp "$ROOT/packaging/linux/sujian.desktop" "$APPDIR/usr/share/applications/sujian.desktop"
cp "$ROOT/packaging/linux/icons/hicolor/256x256/apps/sujian.png" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps/sujian.png"
cp "$ROOT/packaging/linux/icons/hicolor/scalable/apps/sujian.svg" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps/sujian.svg"

# The fetch script materializes model files and the versioned Runtime library.
cp -a "$BUNDLE_DIR/macbert4csc/." "$APPDIR/usr/share/sujian/models/macbert4csc/"
cp "$BUNDLE_DIR/runtime/libonnxruntime.so."* "$APPDIR/usr/lib/sujian/libonnxruntime.so"

echo "==> 部署 Qt 运行库/插件"
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/sujian-linux-qt" \
    --desktop-file "$APPDIR/usr/share/applications/sujian.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/sujian.png" \
    --plugin qt

# linuxdeploy may generate its own AppRun. Replace it with the project launcher
# so APPDIR-relative model/runtime paths are always exported.
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/bash
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
export APPDIR="${APPDIR:-$HERE}"
# AppDir 的 ELF 在 usr/bin；launcher 的 RPM 默认路径是 usr/libexec。
# 显式传给同一个入口脚本，避免 AppImage 回落到系统 RPM 路径。
export SUJIAN_REAL_BINARY="$APPDIR/usr/bin/sujian-linux-qt"
exec "$APPDIR/usr/libexec/sujian/sujian-launcher.sh" "$@"
EOF
chmod +x "$APPDIR/AppRun"

echo "==> 生成 AppImage"
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$OUT_DIR/素笺写作-x86_64.AppImage"
echo "完成：$OUT_DIR/素笺写作-x86_64.AppImage"
