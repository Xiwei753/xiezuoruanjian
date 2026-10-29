# sujian.spec — 素笺写作 RPM 打包规范
#
# 依赖系统 Qt 6.7+，不捆绑 Qt / QML runtime。
# 这些运行时依赖由 Fedora/RPM 包管理器通过 Requires 解决。
#
# 构建方式：见同目录 build-rpm.sh

# GitHub 直接分发主安装包：快速 zstd 压缩，无需生成未发布的 debuginfo/debugsource 包。
%global _binary_payload w1.zstdio
%global debug_package %{nil}

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
# QtQuick/Controls 由 qt6-qtdeclarative 提供，
# Fedora 没有独立的 qt6-qtquickcontrols2(-devel) 包，不要写这条。
# Issue #900：界面迁移到 KDE Kirigami 6 + Breeze + Kirigami Addons，
# QML 模块属于运行时资源，无需额外 BuildRequires，见下方 Requires。
BuildRequires:  qt6-qtdeclarative-devel >= 6.7
BuildRequires:  qt6-qttools-devel
# Issue #912：KDE 应用配色方案切换桥（KColorSchemeManager）需要 KF6 ColorScheme
# 头文件与链接库，该包不提供 pkg-config，构建脚本按头文件目录探测。
BuildRequires:  kf6-kcolorscheme-devel
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

# Issue #900：全面采用 KDE 官方组件体系。UI 使用 Kirigami 6 + Kirigami
# Addons 的 QML 模块，控件风格与图标依赖 Breeze。这些是运行硬依赖。
Requires:       kf6-kirigami >= 6.0
Requires:       kf6-kirigami-addons >= 1.0
# Issue #912：二进制链接 libKF6ColorScheme.so.6（KColorSchemeManager 桥），
# 需要运行时库；通常随 kf6-kirigami 引入，这里显式声明避免构建环境差异。
Requires:       kf6-kcolorscheme
# Issue #912：桥固定使用 BreezeDark/BreezeLight，这两个配色文件由
# plasma-breeze-common 提供（kf6-kcolorscheme / qqc2-breeze-style 都不含），
# 否则手动深/浅色查不到方案而失效。
Requires:       plasma-breeze-common
Requires:       qqc2-breeze-style
Requires:       breeze-icon-theme

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
# Fedora RPM 自动注入的 Rust 标志默认采用单代码生成单元及完整调试符号。
# GitHub 分发的主安装包不生成 debuginfo：保留其余 Fedora 安全编译标志，
# 仅改用 Cargo Release 常规的并行代码生成与零调试符号。
export RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }-Ccodegen-units=16 -Cdebuginfo=0"
cargo build --release --locked -p sujian-linux-qt

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

# Issue #985：内置 MacBERT4CSC ONNX 资源目录与 ONNX Runtime。模型由
# build-rpm.sh 在原仓库的忽略目录按 manifest 下载校验；运行时只读加载，
# 不复制到用户目录，也不在首次启动时联网下载。
install -dpm 0755 %{buildroot}%{_datadir}/sujian/models/macbert4csc
install -pm 0644 packaging/correction/bundled/macbert4csc/model.onnx \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/model.onnx
install -pm 0644 packaging/correction/bundled/macbert4csc/vocab.txt \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/vocab.txt
install -pm 0644 packaging/correction/bundled/macbert4csc/config.json \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/config.json
install -pm 0644 packaging/correction/bundled/macbert4csc/tokenizer.json \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/tokenizer.json
install -pm 0644 packaging/correction/bundled/macbert4csc/tokenizer_config.json \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/tokenizer_config.json
install -pm 0644 packaging/correction/bundled/macbert4csc/special_tokens_map.json \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/special_tokens_map.json
install -pm 0644 packaging/correction/bundled/macbert4csc/calibrator.json \
    %{buildroot}%{_datadir}/sujian/models/macbert4csc/calibrator.json
install -Dpm 0755 packaging/correction/bundled/runtime/libonnxruntime.so.1.22.0 \
    %{buildroot}%{_libdir}/sujian/libonnxruntime.so.1.22.0
ln -s libonnxruntime.so.1.22.0 %{buildroot}%{_libdir}/sujian/libonnxruntime.so

%files
%license LICENSE third_party/macbert4csc/LICENSE third_party/onnxruntime/LICENSE
%doc README.md packaging/correction/THIRD_PARTY_MODEL_NOTICES.md third_party/macbert4csc/NOTICE third_party/onnxruntime/NOTICE
%{_bindir}/sujian
%{_libexecdir}/sujian/sujian-linux-qt
%{_datadir}/applications/io.github.Xiwei753.Sujian.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.Xiwei753.Sujian.svg
%{_datadir}/metainfo/io.github.Xiwei753.Sujian.metainfo.xml
# Issue #985：内置 MacBERT4CSC ONNX 资源目录、ONNX Runtime 与第三方声明。
%{_datadir}/sujian/models/macbert4csc/
%{_libdir}/sujian/libonnxruntime.so
%{_libdir}/sujian/libonnxruntime.so.1.22.0

%changelog
* Sun Oct 18 2026 Xiwei753 <xiwei753@users.noreply.github.com> - 0.1.0-1
- 校对引擎换代：内置 MacBERT4CSC ONNX 模型与 ONNX Runtime，随包分发 Apache-2.0/MIT 许可与第三方来源说明（Issue #985）
* Thu Oct 15 2026 Xiwei753 <xiwei753@users.noreply.github.com> - 0.1.0-1
- 界面全面迁移到 KDE Kirigami 6 + Breeze + Kirigami Addons（Issue #900）
- 新增运行依赖：kf6-kirigami、kf6-kirigami-addons、qqc2-breeze-style、breeze-icon-theme
* Thu Sep 10 2026 Xiwei753 <xiwei753@users.noreply.github.com> - 0.1.0-1
- Initial RPM package for 素笺写作 0.1.0
- Linux Qt 6.7+ 前端，依赖系统 Qt，不捆绑运行时
