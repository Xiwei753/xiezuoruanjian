# Desktop 应用

本目录包含 Linux desktop 原生客户端，使用 Rust 和 Qt/QML 构建，提供桌面端写作体验。

## 主要文件

| 文件 | 用途 |
|------|------|
| `Cargo.toml` | Rust 项目配置 |
| `build.rs` | 构建脚本 |
| `src/` | Rust 源代码，包含 QObject 绑定 |
| `qml/` | QML 界面文件 |
| `../../docs/TECHNICAL_ROUTE.md` | 技术路线文档 |
| `.gitignore` | Git 忽略规则 |

## 架构说明

客户端**必须**通过 FFI 使用 `core/writer_core` Rust 核心库，严格禁止在 C++ 或 QML 中直接实现工作区格式、保存逻辑或同步功能。UI 使用 Qt 6 / QML 构建，所有业务逻辑依赖 Rust 核心。

路线收口：`apps/desktop` 当前定位为 Linux Qt/QML 客户端，优先稳定 Linux 输入法、渲染、动画、AppImage、日志导出和 runtime profile；不拆分 `apps/linux` / `apps/windows`。Windows 后续不继续当前 Qt 桌面补丁路线，待 Linux 输入法和动画稳定后另开原生 Windows 客户端路线。

## 依赖关系

- 依赖 `core/writer_core` Rust 核心库
- 需要 Qt 6 开发环境，Linux 二进制不应再链接 `libQt5Core` / `libQt5Qml` / `libQt5Quick`
- 需要支持 C++17 的 C++ 编译器；Linux Qt 绑定会在 `build.rs` 中强制给 `cpp_build` 传入 `-std=c++17`
- GitHub Actions 也只安装 Qt6 依赖；本地和 CI 不再维护 Qt5 构建链路

## 使用说明

### 安装依赖

**Fedora / openSUSE：**
```bash
sudo dnf install gcc-c++ qt6-qtbase qt6-qtdeclarative qt6-qtquickcontrols2 qt6-qttools qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtquickcontrols2-devel qt6-qttools-devel
```

**Ubuntu / Debian：**
```bash
sudo apt install g++ qt6-base-dev qt6-declarative-dev qt6-tools-dev qt6-tools-dev-tools qml6-module-qtquick qml6-module-qtquick-controls qml6-module-qtquick-window
```

### 构建运行

```bash
bash start.sh
```

`start.sh` 会自动优先检测 Qt6 qmake，并设置 Qt6 QML/plugin 路径：

```bash
QML2_IMPORT_PATH=/usr/lib64/qt6/qml
QT_PLUGIN_PATH=/usr/lib64/qt6/plugins
```

仓库根目录的 `.cargo/config.toml` 默认把 Linux 构建指向 Fedora Qt6 开发路径：`/usr/include/qt6` 和 `/usr/lib64`。如果你的发行版 Qt6 安装在其他位置，可以在执行 `cargo` 前显式覆盖 `QT_INCLUDE_PATH`、`QT_LIBRARY_PATH` 或 `QMAKE`。

不要把 `/usr/lib64/qt5/qml` 或 `/usr/lib64/qt5/plugins` 混入启动路径，否则 Qt5 程序和 Qt6 QML 模块会互相污染。构建完成后可用以下命令确认链接结果：

```bash
ldd target/debug/sujian-desktop | grep -Ei 'Qt5|Qt6|qml|quick'
```

结果应出现 `libQt6Core`、`libQt6Qml`、`libQt6Quick`，不应出现 `libQt5Core`、`libQt5Qml`、`libQt5Quick`。

自研写作区使用 Rust 自绘渲染，完全替代了传统的 QML `TextArea`，提供更可控的编辑体验。当前 `SujianEditorItem` 为唯一受支持的编辑器实现。

吐字动画已可通过 Core transaction 与 animation_events_json 驱动，由 QML EditorAnimationOverlay 负责叠加渲染。
