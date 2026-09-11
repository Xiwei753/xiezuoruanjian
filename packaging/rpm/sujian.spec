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
BuildRequires:  qt6-qtdeclarative-devel >= 6.7
BuildRequires:  qt6-qtquickcontrols2-devel >= 6.7
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
Requires:       qt6-qtquickcontrols2 >= 6.7

%description
素笺写作是一款跨平台写作软件，Linux 端基于 Qt 6.7+ Quick 和自研文本渲染。
核心业务逻辑使用 Rust 实现，通过 UniFFI 暴露稳定接口，保证多平台行为一致。
本包不捆绑 Qt 或 QML runtime，这些由系统包管理器通过依赖解决。

%prep
%setup -q

%build
# 构建 Linux Qt 前端二进制（产物名 sujian-linux-qt）
cargo build --release --locked --offline -p sujian-linux-qt

%install
# 主二进制：安装为 /usr/bin/sujian
install -Dpm 0755 target/release/sujian-linux-qt \
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
%{_datadir}/applications/io.github.Xiwei753.Sujian.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.Xiwei753.Sujian.svg
%{_datadir}/metainfo/io.github.Xiwei753.Sujian.metainfo.xml

%changelog
* Thu Sep 10 2026 Xiwei753 <xiwei753@users.noreply.github.com> - 0.1.0-1
- Initial RPM package for 素笺写作 0.1.0
- Linux Qt 6.7+ 前端，依赖系统 Qt，不捆绑运行时
