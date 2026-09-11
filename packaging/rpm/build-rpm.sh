#!/bin/bash
#
# build-rpm.sh — 素笺写作 RPM 打包脚本
#
# 在顶层工作目录创建 RPM 构建环境，staging 源码并打包为 RPM。
# 不捆绑 Qt / QML runtime，运行时依赖由 spec 的 Requires 解决。
# 实际编译发生在 spec 的 %build，使用 --locked --offline -p sujian-linux-qt。
#
# 用法:
#   ./packaging/rpm/build-rpm.sh
#
# 依赖 (Fedora/RHEL):
#   sudo dnf install rpm-build rpmdevtools \
#       qt6-qtbase-devel qt6-qtdeclarative-devel \
#       qt6-qtquickcontrols2-devel qt6-qttools-devel \
#       rust cargo gcc-c++ cmake make pkgconf-pkg-config openssl-devel
#
set -euo pipefail

# 解析脚本所在目录与仓库根目录（脚本位于 packaging/rpm/，向上两级）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# 固定并导出 Cargo 输出目录，使昂贵的 Rust Release 编译产物进入可复用的
# target/rpm-cargo，而非 rpmbuild 每次新建即删的 BUILD 目录。
# 这样本机连续打 RPM 与远端 CI 都能复用同一份编译结果。
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${REPO_ROOT}/target/rpm-cargo}"
export CARGO_TARGET_DIR
mkdir -p "${CARGO_TARGET_DIR}"

NAME="sujian"
VERSION="0.1.0"
RELEASE="1"

echo "==> 仓库根目录: ${REPO_ROOT}"
echo "==> 包: ${NAME}-${VERSION}-${RELEASE}"

# ---------------------------------------------------------------------------
# 1. 依赖检查与安装提示
# ---------------------------------------------------------------------------
if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "错误: 未找到 rpmbuild，请先安装 RPM 构建工具。" >&2
    echo "" >&2
    echo "  Fedora/RHEL:" >&2
    echo "    sudo dnf install rpm-build rpmdevtools" >&2
    echo "  构建依赖:" >&2
    echo "    sudo dnf install qt6-qtbase-devel qt6-qtdeclarative-devel \\" >&2
    echo "        qt6-qtquickcontrols2-devel qt6-qttools-devel \\" >&2
    echo "        rust cargo gcc-c++ cmake make pkgconf-pkg-config openssl-devel" >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "错误: 未找到 cargo，请先安装 Rust 工具链。" >&2
    echo "  Fedora/RHEL: sudo dnf install rust cargo" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# 2. 准备 RPM 构建目录树
# ---------------------------------------------------------------------------
RPM_TOPDIR="${REPO_ROOT}/target/rpm-build"
echo "==> 准备 RPM 构建目录: ${RPM_TOPDIR}"
mkdir -p "${RPM_TOPDIR}/BUILD" \
         "${RPM_TOPDIR}/RPMS" \
         "${RPM_TOPDIR}/SOURCES" \
         "${RPM_TOPDIR}/SPECS" \
         "${RPM_TOPDIR}/SRPMS"

# ---------------------------------------------------------------------------
# 3. 源码 staging：复制到临时目录，执行 cargo vendor
# ---------------------------------------------------------------------------
STAGING_DIR="${RPM_TOPDIR}/staging/${NAME}-${VERSION}"
echo "==> Staging 源码到: ${STAGING_DIR}"
rm -rf "${STAGING_DIR}"
mkdir -p "${STAGING_DIR}"

# 复制源码（排除构建产物和 VCS 目录）
tar -cf - \
    --exclude="./rpm-build" \
    --exclude="./target" \
    --exclude="./.git" \
    --exclude="./.cargo" \
    -C "${REPO_ROOT}" . | tar -xf - -C "${STAGING_DIR}"

# 在 staging 目录内执行 cargo vendor
echo "==> cargo vendor --locked vendor"
mkdir -p "${STAGING_DIR}/vendor"
cargo vendor --locked "${STAGING_DIR}/vendor" \
    --manifest-path "${STAGING_DIR}/Cargo.toml" > /dev/null

# 写临时 .cargo/config.toml，将 crates.io 指向本地 vendor
mkdir -p "${STAGING_DIR}/.cargo"
cat > "${STAGING_DIR}/.cargo/config.toml" <<'VENDOR_TOML'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
VENDOR_TOML

# ---------------------------------------------------------------------------
# 4. 从 staging 目录生成 Source0 tarball
# ---------------------------------------------------------------------------
TARBALL="${RPM_TOPDIR}/SOURCES/${NAME}-${VERSION}.tar.gz"
echo "==> 生成源码 tarball: ${TARBALL}"
tar -czf "${TARBALL}" \
    --transform "s,^\.,${NAME}-${VERSION}," \
    -C "${STAGING_DIR}" .

# ---------------------------------------------------------------------------
# 5. 复制 spec 文件到 SPECS 目录
# ---------------------------------------------------------------------------
cp "${SCRIPT_DIR}/sujian.spec" "${RPM_TOPDIR}/SPECS/${NAME}.spec"

# ---------------------------------------------------------------------------
# 6. 执行 rpmbuild -bb（仅构建二进制 RPM）
# ---------------------------------------------------------------------------
echo "==> rpmbuild -bb"
rpmbuild -bb \
    --define "_topdir ${RPM_TOPDIR}" \
    "${RPM_TOPDIR}/SPECS/${NAME}.spec"

# ---------------------------------------------------------------------------
# 7. 输出产物路径并复制到 dist/rpm/
# ---------------------------------------------------------------------------
DIST_DIR="${REPO_ROOT}/dist/rpm"
mkdir -p "${DIST_DIR}"
echo "==> 完成。RPM 产物位于: ${RPM_TOPDIR}/RPMS"
find "${RPM_TOPDIR}/RPMS" -name "*.rpm" -print
find "${RPM_TOPDIR}/RPMS" -name "*.rpm" -exec cp {} "${DIST_DIR}/" \;
echo "==> RPM 已复制到: ${DIST_DIR}"
