#!/bin/bash
#
# build-rpm.sh — 素笺写作 RPM 打包脚本
#
# 在顶层工作目录创建 RPM 构建环境，构建 release 二进制并打包为 RPM。
# 不捆绑 Qt / fcitx5-qt / QML runtime，运行时依赖由 spec 的 Requires 解决。
#
# 用法:
#   ./packaging/rpm/build-rpm.sh
#
# 依赖 (Fedora/RHEL):
#   sudo dnf install rpm-build rpmdevtools \
#       qt6-qtbase-devel qt6-qtdeclarative-devel \
#       qt6-qtquickcontrols2-devel qt6-qttools-devel \
#       rust cargo gcc-c++ cmake make
#
set -euo pipefail

# 解析脚本所在目录与仓库根目录（脚本位于 packaging/rpm/，向上两级）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

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
    echo "        rust cargo gcc-c++ cmake make" >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "错误: 未找到 cargo，请先安装 Rust 工具链。" >&2
    echo "  Fedora/RHEL: sudo dnf install rust cargo" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# 2. 构建 release 二进制
# ---------------------------------------------------------------------------
echo "==> cargo build --release"
cargo build --release --manifest-path "${REPO_ROOT}/Cargo.toml"

BINARY="${REPO_ROOT}/target/release/sujian-linux-qt"
if [[ ! -x "${BINARY}" ]]; then
    echo "错误: 构建产物不存在: ${BINARY}" >&2
    exit 1
fi
echo "==> 构建产物: ${BINARY}"

# ---------------------------------------------------------------------------
# 3. 准备 RPM 构建目录树（在仓库根目录下）
# ---------------------------------------------------------------------------
RPM_TOPDIR="${REPO_ROOT}/rpm-build"
echo "==> 准备 RPM 构建目录: ${RPM_TOPDIR}"
mkdir -p "${RPM_TOPDIR}/BUILD" \
         "${RPM_TOPDIR}/RPMS" \
         "${RPM_TOPDIR}/SOURCES" \
         "${RPM_TOPDIR}/SPECS" \
         "${RPM_TOPDIR}/SRPMS}"

# ---------------------------------------------------------------------------
# 4. 打包源码 tarball（排除构建产物与 VCS 目录）
# ---------------------------------------------------------------------------
TARBALL="${RPM_TOPDIR}/SOURCES/${NAME}-${VERSION}.tar.gz"
echo "==> 生成源码 tarball: ${TARBALL}"
tar -czf "${TARBALL}" \
    --transform "s,^\./,${NAME}-${VERSION}/," \
    --exclude="./rpm-build" \
    --exclude="./target" \
    --exclude="./.git" \
    --exclude="./.cargo" \
    -C "${REPO_ROOT}" .

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
# 7. 输出产物路径
# ---------------------------------------------------------------------------
echo "==> 完成。RPM 产物位于: ${RPM_TOPDIR}/RPMS"
find "${RPM_TOPDIR}/RPMS" -name "*.rpm" -print
