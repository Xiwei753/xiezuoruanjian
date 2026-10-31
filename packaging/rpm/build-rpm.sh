#!/bin/bash
#
# build-rpm.sh — 素笺写作 RPM 打包脚本
#
# 直接从仓库工作目录构建 RPM，复用 Cargo target 缓存。
# 不捆绑 Qt / QML runtime，运行时依赖由 spec 的 Requires 解决。
# 实际编译发生在 spec 的 %build，使用 --locked -p sujian-linux-qt。
#
# 用法:
#   ./packaging/rpm/build-rpm.sh
#
# 依赖 (Fedora/RHEL):
#   sudo dnf install rpm-build python3 \
#       qt6-qtbase-devel qt6-qtdeclarative-devel \
#       qt6-qttools-devel kf6-kcolorscheme-devel \
#       rust cargo gcc-c++ cmake make pkgconf-pkg-config openssl-devel
#
# 运行时依赖 (由 spec 的 Requires 拉取，供本地运行/验证时手动安装):
#   Fedora/RHEL: qt6-qtwayland kf6-kirigami kf6-kirigami-addons kf6-kcolorscheme \
#                plasma-breeze-common qqc2-breeze-style breeze-icon-theme
#   Debian/Ubuntu: kirigami2 kirigami-addons qqc2-breeze-style breeze-icon-theme
#
set -euo pipefail

# 解析脚本所在目录与仓库根目录（脚本位于 packaging/rpm/，向上两级）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# 固定并导出 Cargo 输出目录，原工作区编译的 Rust Release 产物
# 可由本地连续构建与 CI 的 rust-cache 直接复用。
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${REPO_ROOT}/target/rpm-cargo}"
export CARGO_TARGET_DIR
mkdir -p "${CARGO_TARGET_DIR}"

NAME="sujian"
VERSION="1.0.0"
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
    echo "    sudo dnf install rpm-build python3" >&2
    echo "  构建依赖:" >&2
    echo "    sudo dnf install qt6-qtbase-devel qt6-qtdeclarative-devel \\" >&2
    echo "        qt6-qttools-devel kf6-kcolorscheme-devel \\" >&2
    echo "        rust cargo gcc-c++ cmake make pkgconf-pkg-config openssl-devel" >&2
    echo "  运行时依赖 (运行已打好的包时需要, 由 spec Requires 声明):" >&2
    echo "    sudo dnf install qt6-qtwayland kf6-kirigami kf6-kirigami-addons \\" >&2
    echo "        kf6-kcolorscheme plasma-breeze-common qqc2-breeze-style breeze-icon-theme" >&2
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
# 3. 获取并校验内置 MacBERT4CSC ONNX 模型与 ONNX Runtime
# ---------------------------------------------------------------------------
# 资源存放在仓库现有的忽略目录；rpmbuild --build-in-place 的 %install
# 将直接从此目录安装到包内，不再复制整份源码和生成 Source0 tarball。
BUNDLED_MODEL_DIR="${REPO_ROOT}/packaging/correction/bundled"
echo "==> 获取内置校对模型与 ONNX Runtime 到: ${BUNDLED_MODEL_DIR}"
SUJIAN_FETCH_ORT=1 bash "${SCRIPT_DIR}/../correction/fetch-model.sh" "${BUNDLED_MODEL_DIR}"

# ---------------------------------------------------------------------------
# 5.5. 计算并导出构建身份环境变量
# ---------------------------------------------------------------------------
# 显式传递 SHA 给 rpmbuild/Cargo，确保本地与 GitHub Actions 的构建身份一致。
# Issue #710 评论 5731145076: git sha 不能再正常产出 unknown。
# 优先使用显式传入值；GitHub Actions 直接使用 GITHUB_SHA，避免容器挂载仓库
# 因 safe.directory / ownership 导致 git rev-parse 失败；本机构建再回退 git。
# 无法取得有效 SHA 时仍直接失败，正式 RPM 不允许 fallback 到 unknown。
if [ -z "${SUJIAN_GIT_COMMIT_SHA:-}" ] || [ "${SUJIAN_GIT_COMMIT_SHA}" = "unknown" ]; then
    if [ -n "${GITHUB_SHA:-}" ]; then
        SUJIAN_GIT_COMMIT_SHA="${GITHUB_SHA:0:7}"
    elif ! SUJIAN_GIT_COMMIT_SHA="$(git -C "${REPO_ROOT}" rev-parse --short HEAD)"; then
        echo "错误: 无法从 git 仓库获取有效的 commit SHA。" >&2
        echo "  仓库根目录: ${REPO_ROOT}" >&2
        echo "  RPM 正式包必须有有效 git sha 以便追源码，不允许 fallback 到 unknown。" >&2
        exit 1
    fi
fi

if [ -z "${SUJIAN_GIT_COMMIT_SHA}" ] || [ "${SUJIAN_GIT_COMMIT_SHA}" = "unknown" ]; then
    echo "错误: 构建身份中的 commit SHA 无效。" >&2
    exit 1
fi
export SUJIAN_GIT_COMMIT_SHA
SUJIAN_PACKAGE_TYPE="rpm"
export SUJIAN_PACKAGE_TYPE
echo "==> SUJIAN_GIT_COMMIT_SHA=${SUJIAN_GIT_COMMIT_SHA}"
echo "==> SUJIAN_PACKAGE_TYPE=${SUJIAN_PACKAGE_TYPE}"

# ---------------------------------------------------------------------------
# 4. 原仓库目录编译、打包（仅构建二进制 RPM）
# ---------------------------------------------------------------------------
# --build-in-place 跳过 %prep，%build / %install 在当前源码目录执行。
# --noclean 保留构建工作目录；%buildroot 仍位于独立的 RPM 构建目录。
cd "${REPO_ROOT}"
echo "==> rpmbuild --build-in-place -bb"
rpmbuild --build-in-place --noclean -bb \
    --define "_topdir ${RPM_TOPDIR}" \
    "${SCRIPT_DIR}/sujian.spec"

# ---------------------------------------------------------------------------
# 5. 只把用户实际安装需要的主 RPM 复制到 dist/rpm/
# ---------------------------------------------------------------------------
DIST_DIR="${REPO_ROOT}/dist/rpm"
mkdir -p "${DIST_DIR}"
find "${DIST_DIR}" -maxdepth 1 -type f -name "*.rpm" -delete

echo "==> 完成。RPM 产物位于: ${RPM_TOPDIR}/RPMS"
find "${RPM_TOPDIR}/RPMS" -name "*.rpm" -print

mapfile -t MAIN_RPMS < <(
    find "${RPM_TOPDIR}/RPMS" -type f \
        -name "${NAME}-${VERSION}-${RELEASE}*.rpm" \
        ! -name "*-debuginfo-*" \
        ! -name "*-debugsource-*" \
        -print
)

if [ "${#MAIN_RPMS[@]}" -ne 1 ]; then
    echo "错误: 期望恰好一个 sujian 主 RPM，实际找到 ${#MAIN_RPMS[@]} 个。" >&2
    find "${RPM_TOPDIR}/RPMS" -type f -name "*.rpm" -print >&2
    exit 1
fi

cp "${MAIN_RPMS[0]}" "${DIST_DIR}/"
echo "==> 主 RPM 已复制到: ${DIST_DIR}/$(basename "${MAIN_RPMS[0]}")"
