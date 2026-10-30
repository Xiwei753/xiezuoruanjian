#!/usr/bin/env bash
# 在 Debian 13 (trixie) 构建，使用系统 Qt 6.8/KF6。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="$(awk '/^\[package\]/{in_package=1;next} in_package && /^version = /{gsub(/"/,"",$3);print $3;exit}' "$ROOT/apps/Linux_qt/Cargo.toml")"
[[ -n "$VERSION" ]] || { echo "无法读取 Linux Qt 包版本" >&2; exit 1; }
ARCH="$(dpkg --print-architecture)"
[[ "$ARCH" == amd64 ]] || { echo "目前只构建 amd64 DEB" >&2; exit 1; }

DIST="$ROOT/dist/deb"
STAGE="$DIST/stage"
bash "$ROOT/packaging/linux/stage-native-package.sh" "$STAGE" deb
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: sujian
Version: ${VERSION}-1
Section: editors
Priority: optional
Architecture: ${ARCH}
Maintainer: Xiwei753 <xiwei753@users.noreply.github.com>
Homepage: https://github.com/Xiwei753/xiezuoruanjian
Installed-Size: $(du -sk "$STAGE/usr" | cut -f1)
Depends: bubblewrap, libc6, libstdc++6, libgcc-s1, libssl3t64, libqt6core6t64 (>= 6.7), libqt6gui6 (>= 6.7), libqt6qml6 (>= 6.7), libqt6quick6 (>= 6.7), libkf6colorscheme6, qt6-wayland, qml6-module-qtqml, qml6-module-qtquick, qml6-module-qtquick-controls, qml6-module-qtquick-layouts, qml6-module-qtquick-templates, qml6-module-qtquick-dialogs, qml6-module-org-kde-kirigami, qml6-module-org-kde-kirigamiaddons-components, qml6-module-org-kde-kirigamiaddons-formcard, qml6-module-org-kde-desktop, breeze, breeze-icon-theme
Recommends: fcitx5-frontend-qt6
Description: Sujian cross-platform writing application
 A native Linux Qt6 and Kirigami application with a Rust core.
 Includes the offline Chinese correction model and ONNX Runtime.
EOF
# Debian 软件包使用 /usr/share/doc/<pkg>/copyright 记录主软件及第三方许可。
cp "$ROOT/LICENSE" "$STAGE/usr/share/doc/sujian/copyright"
# zstd 1 级：直接分发包优先快速打包，保留 dpkg 原生压缩格式。
dpkg-deb --build --root-owner-group --compression=zstd --compression-level=1 \
  "$STAGE" "$DIST/sujian_${VERSION}-1_${ARCH}.deb"
echo "DEB: $DIST/sujian_${VERSION}-1_${ARCH}.deb"
