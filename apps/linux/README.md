# Linux 应用

本目录包含 Linux 原生客户端，使用 Rust 和 Qt/QML 构建，提供桌面端写作体验。

## 主要文件

| 文件 | 用途 |
|------|------|
| `Cargo.toml` | Rust 项目配置 |
| `build.rs` | 构建脚本 |
| `src/` | Rust 源代码，包含 QObject 绑定 |
| `qml/` | QML 界面文件 |
| `TECHNICAL_ROUTE.md` | 技术路线文档 |
| `.gitignore` | Git 忽略规则 |

## 架构说明

客户端**必须**通过 FFI 使用 `core/writer_core` Rust 核心库，严格禁止在 C++ 或 QML 中直接实现工作区格式、保存逻辑或同步功能。UI 使用 Qt 5 / QML 构建，所有业务逻辑依赖 Rust 核心。

## 依赖关系

- 依赖 `core/writer_core` Rust 核心库
- 需要 Qt 5 开发环境

## 使用说明

### 安装依赖

**Fedora / openSUSE：**
```bash
sudo dnf install qt5-qtbase-devel qt5-qtdeclarative-devel qt5-qtquickcontrols2-devel qt5-qtwayland
```

**Ubuntu / Debian：**
```bash
sudo apt install qtbase5-dev qtdeclarative5-dev qtquickcontrols2-5-dev qtwayland5
```

### 构建运行

```bash
cargo run -p linux
```

如果系统的 `qmake` 命名为 `qmake-qt5`：
```bash
QMAKE=/usr/bin/qmake-qt5 cargo run -p linux
```