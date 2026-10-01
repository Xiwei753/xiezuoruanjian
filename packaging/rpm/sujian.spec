# sujian.spec — 素笺写作 RPM 打包规范
#
# 依赖系统 Qt 6.7+，不捆绑 Qt / QML runtime。
# 这些运行时依赖由 Fedora/RPM 包管理器通过 Requires 解决。
#
# 构建方式：见同目录 build-rpm.sh

Name:           sujian
Version:        0.1.0
Release:        1%{?dist}
Summary:        素笺写作 — 跨平台写作软件

# 仓库根目录 LICENSE 为 GNU GPL v3，此处使用 SPDX 表达式。
License:        GPL-3.0-only
URL:            https://github.com/Xiwei753/xiezuoruanjian
Source0:        %{name}-%{version}.tar.gz

# 构建依赖：Qt 6.7+ 开发包 + Rust 工具链 + 基础构建工具
BuildRequires:  qt6-qtbase-devel >= 6.7
# QtQuick/Controls 与 QtQuick.Controls.Material 由 qt6-qtdeclarative 提供，
# Fedora 没有独立的 qt6-qtquickcontrols2(-devel) 包，不要写这条。
BuildRequires:  qt6-qtdeclarative-devel >= 6.7
BuildRequires:  qt6-qttools-devel
BuildRequires:  rust
BuildRequires:  cargo
BuildRequires:  gcc-c++
BuildRequires:  cmake
BuildRequires:  make
BuildRequires:  openssl-devel
BuildRequires:  pkgconf-pkg-config

# 运行时依赖：系统 Qt 6.7+ 运行库
Requires:       qt6-qtbase >= 6.7
Requires:       qt6-qtdeclarative >= 6.7
# Issue #729 评论 5762596831 第 1 部分：收口到原生 Wayland 运行环境
# qt6-qtwayland 提供 Wayland QPA 插件，避免回退到 xcb/XWayland；
# fcitx5-qt6 提供 fcitx5 Qt6 输入法插件，供中文输入法在 Wayland 下工作。
# Issue #729 评论 5763441474：fcitx5-qt6 改为 Recommends（软依赖），
# 用户未安装 fcitx5 时不强制拉入，Wayland 原生 text-input 协议仍可用。
Requires:       qt6-qtwayland
Recommends:     fcitx5-qt6

%description
素笺写作是一款跨平台写作软件，Linux 端基于 Qt 6.7+ Quick 和自研文本渲染。
核心业务逻辑使用 Rust 实现，通过 UniFFI 暴露稳定接口，保证多平台行为一致。
本包不捆绑 Qt 或 QML runtime，这些由系统包管理器通过依赖解决。

%prep
%setup -q

%build
# 接收外部传入的构建身份环境变量（由 build-rpm.sh 在仓库根目录计算）
export SUJIAN_GIT_COMMIT_SHA="${SUJIAN_GIT_COMMIT_SHA:-unknown}"
export SUJIAN_PACKAGE_TYPE="${SUJIAN_PACKAGE_TYPE:-rpm}"
# 构建 Linux Qt 前端二进制（产物名 sujian-linux-qt）
# 支持外部 CARGO_TARGET_DIR：构建脚本传持久目录时走可复用缓存，
# 普通 rpmbuild 未传时回退到源码目录自身的 target。
: "${CARGO_TARGET_DIR:=target}"
export CARGO_TARGET_DIR
cargo build --release --locked --offline -p sujian-linux-qt

%install
# Issue #803 评论 5904340835：启动诊断层
# 真实 Rust/Qt ELF 装到 libexec，launcher 装到 bin。
# /usr/bin/sujian 现在是启动器脚本，负责创建诊断目录并镜像真实二进制
# 的 stdout/stderr 到 ~/.sujianxiezuo/diagnostics/startup/，即使二进制
# 因缺动态库/Qt platform plugin/ELF loader 在 main() 前退出也能留下日志。
: "${CARGO_TARGET_DIR:=target}"
# 真实 ELF 装到 libexec
install -Dpm 0755 "${CARGO_TARGET_DIR}/release/sujian-linux-qt" \
    %{buildroot}%{_libexecdir}/sujian/sujian-linux-qt
# launcher 装到 bin
install -Dpm 0755 packaging/linux/sujian-launcher.sh \
    %{buildroot}%{_bindir}/sujian

# Desktop entry
install -Dpm 0644 packaging/rpm/io.github.Xiwei753.Sujian.desktop \
    %{buildroot}%{_datadir}/applications/io.github.Xiwei753.Sujian.desktop

# 图标：复用仓库内 SVG，按应用 ID 命名安装到 hicolor scalable 目录
install -Dpm 0644 packaging/linux/icons/hicolor/scalable/apps/sujian.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.Xiwei753.Sujian.svg

# AppStream metainfo
install -Dpm 0644 packaging/rpm/io.github.Xiwei753.Sujian.metainfo.xml \
    %{buildroot}%{_datadir}/metainfo/io.github.Xiwei753.Sujian.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/sujian
%{_libexecdir}/sujian/sujian-linux-qt
%{_datadir}/applications/io.github.Xiwei753.Sujian.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.Xiwei753.Sujian.svg
%{_datadir}/metainfo/io.github.Xiwei753.Sujian.metainfo.xml

%changelog
* Thu Sep 10 2026 Xiwei753 <xiwei753@users.noreply.github.com> - 0.1.0-1
- Initial RPM package for 素笺写作 0.1.0
- Linux Qt 6.7+ 前端，依赖系统 Qt，不捆绑运行时
