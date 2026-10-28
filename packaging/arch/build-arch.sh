#!/usr/bin/env bash
# 通过非 root makepkg 生成 Arch 原生 .pkg.tar.zst。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST="$ROOT/dist/arch"
STAGE="$DIST/stage"
WORK="$DIST/makepkg"

bash "$ROOT/packaging/linux/stage-native-package.sh" "$STAGE" arch
mkdir -p "$WORK"
cp "$ROOT/packaging/arch/PKGBUILD" "$WORK/PKGBUILD"
VERSION="$(awk '/^\[package\]/{in_package=1;next} in_package && /^version = /{gsub(/"/,"",$3);print $3;exit}' "$ROOT/apps/Linux_qt/Cargo.toml")"
[[ -n "$VERSION" ]] || { echo "无法读取 Linux Qt 包版本" >&2; exit 1; }
sed -i "s/^pkgver=.*/pkgver=$VERSION/" "$WORK/PKGBUILD"

# makepkg 明确禁止 root 运行；编译已在 Arch 容器内完成。
id sujianpkg >/dev/null 2>&1 || useradd -m sujianpkg
chown -R sujianpkg:sujianpkg "$DIST"
(
  cd "$WORK"
  runuser -u sujianpkg -- makepkg --nodeps --noconfirm --force
)
mapfile -t PACKAGES < <(find "$WORK" -maxdepth 1 -type f -name "sujian-$VERSION-*.pkg.tar.zst" -print)
[[ "${#PACKAGES[@]}" -eq 1 ]] || { echo "无法定位唯一的 Arch 安装包" >&2; exit 1; }
cp "${PACKAGES[0]}" "$DIST/"
echo "Arch: $DIST/$(basename "${PACKAGES[0]}")"
