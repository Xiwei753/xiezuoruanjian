# 素笺写作

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Linux Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml)
[![Android Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml)
[![Windows Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/windows_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/windows_build.yml)

<!-- 截图占位：替换为实际应用截图 -->
<!--
![素笺写作 - 写作界面](docs/screenshots/writing.png)
![素笺写作 - 星图](docs/screenshots/starmap.png)
-->

一款面向小说创作者的跨平台写作工具，采用 Rust 核心 + 各平台原生客户端架构，确保数据安全与写作体验。

## 特性

- 纯文本写作，数据永远属于你
- 项目 / 卷 / 章节三级组织结构
- 自动保存，安心创作
- 一键排版，规范格式
- 星图（StarMap）角色关系可视化
- AI 写作辅助
- 云同步（Beta）
- 跨平台：Android / Linux / HarmonyOS

## 架构

```
core/writer_core/     Rust 核心库（唯一业务逻辑层）
apps/android/         Kotlin Android 客户端
apps/desktop/         Qt6/QML Linux 客户端
apps/harmony/         ArkTS HarmonyOS NEXT 客户端
bindings/             跨平台绑定代码
```

- `core/writer_core`: 使用 Rust 编写的**唯一**业务底层核心库。处理所有文件 I/O、项目管理、同步、格式化和设置规则。严格排除 UI 逻辑。
- `apps/android`: 原生 Kotlin Android 客户端。主业务入口为 `AppServiceBridge + UniFFI`，`BridgeProvider` 只暴露领域 Bridge 给 Repository/ViewModel/UI 做平台适配。
- `apps/desktop`: 原生 Rust + Qt6/QML Linux 客户端。UI 通过 QObject 后端适配层调用 Rust Core，不允许在 QML 中实现工作区、保存或同步规则。
- `apps/harmony`: 原生 ArkTS HarmonyOS NEXT 客户端。通过 NAPI C++ 桥接层调用 Rust Core FFI，ArkTS 侧通过 `IWriterCoreBridge` 接口解耦。
- `bindings`: 用于连接 Rust 核心和原生客户端的代码。

> 注：原有的 Flutter 客户端由于其架构冲突已彻底移除。

详情请参阅 [《技术路线与架构约束》](docs/TECHNICAL_ROUTE.md)。

## 核心原则

- **唯一业务底层**：Rust 核心 (`core/writer_core`) 是文件和数据逻辑的唯一入口。客户端不允许自行拼接路径、处理 Workspace 规则。
- **单一事实来源**：`docs/workspace_format.md` 定义了工作区格式。它由 Rust Core 维护和操作。
- **客户端做薄**：客户端仅负责 UI 渲染、导航、输入法交互和主题。不应包含任何持久化和数据逻辑。

## 开发

### 工具

- `tools/build_core.sh`: 构建 Rust 核心库。
- `scripts/generate_icons.py`: 从 `assets/brand/icon/source` 生成各平台应用图标资源。

### Rust 核心

```bash
cd core/writer_core
cargo fmt
cargo check
cargo test
```

### Android 客户端

```bash
./tools/build_android.sh
```

**支持目标说明**：官方只支持 `arm64-v8a` 构建。不支持 `x86_64` Android 设备或模拟器。如需支持 `x86_64`，开源用户可自行修改 `tools/build_android.sh` 和 `apps/android/app/build.gradle.kts` 添加对应 ABI。

### Linux 客户端

Linux 客户端统一使用 Qt6。CI、`apps/desktop/build.rs` 和目录 README 都以 Qt6 为唯一构建链路；不要混入 Qt5 QML/plugin 路径。

```bash
cargo run -p sujian-desktop
```

If you encounter rendering issues (such as double UI or black screen misalignment under Wayland), try running with basic render loop and enabling debug logs:

```bash
QSG_INFO=1 QSG_RENDER_LOOP=basic cargo run -p sujian-desktop
```

### HarmonyOS 客户端

使用 DevEco Studio 打开 `apps/harmony/` 目录。需要先构建预编译 Rust FFI 库：

```bash
./tools/build_harmony.sh
```

要求：
- Rust 工具链添加 `aarch64-unknown-linux-ohos` target
- 设置 `OHOS_NDK_HOME` 环境变量

### 实机测试验证步骤

- 新建作品。
- 进入作品，确认自动出现“第一卷”。
- 点击新建章节，并输入内容。
- 确认自动保存。
- 退出后重新进入该章节，确认内容仍在。

## 文档

- `docs/TECHNICAL_ROUTE.md`: 全局技术路线与架构约束
- `docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md`: 跨平台能力契约与 Core-first 架构约束
- `docs/API_CONTRACTS.md`: 接口边界与交互契约 (Bridge, Backend, QML)
- `docs/PRODUCT_DESIGN.md`: 素笺产品设计与视觉契约 (产品定位, 莫奈取色, 星图, 设置设计)
- `docs/workspace_format.md`: 磁盘上文档结构的单一事实来源
- `docs/settings_schema.md`: 本地和可同步设置定义
- `docs/sync_rules.md`: 同步规则
- `docs/starmap_semantics.md`: 星图语义地基 (独立对象与引用安全)
- `docs/editor_engine_route.md`: 自绘编辑器与统一事件层路线
- `docs/desktop_ime_notes.md`: Desktop (Linux) 输入法笔记

---

The following English content is machine-translated

# Sujian Writer

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Linux Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml)
[![Android Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml)
[![Windows Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/windows_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/windows_build.yml)

<!-- Screenshot placeholder: replace with actual app screenshots -->
<!--
![Sujian Writer - Writing View](docs/screenshots/writing.png)
![Sujian Writer - StarMap](docs/screenshots/starmap.png)
-->

A cross-platform writing tool for novel creators, built with a Rust core + native client architecture to ensure data safety and writing experience.

## Features

- Plain text writing — your data always belongs to you
- Project / Volume / Chapter three-level organization
- Auto-save for peace of mind
- One-click formatting
- StarMap character relationship visualization
- AI writing assistant
- Cloud sync (Beta)
- Cross-platform: Android / Linux / HarmonyOS

## Architecture

```
core/writer_core/     Rust core library (sole business logic layer)
apps/android/         Kotlin Android client
apps/desktop/         Qt6/QML Linux client
apps/harmony/         ArkTS HarmonyOS NEXT client
bindings/             Cross-platform binding code
```

- `core/writer_core`: The **sole** business logic core library written in Rust. Handles all file I/O, project management, sync, formatting, and settings rules. Strictly excludes UI logic.
- `apps/android`: Native Kotlin Android client. Main business entry point is `AppServiceBridge + UniFFI`. `BridgeProvider` only exposes domain Bridges to Repository/ViewModel/UI for platform adaptation.
- `apps/desktop`: Native Rust + Qt6/QML Linux client. UI calls Rust Core through the QObject backend adapter layer. Implementing workspace, save, or sync rules in QML is not allowed.
- `apps/harmony`: Native ArkTS HarmonyOS NEXT client. Calls Rust Core FFI through the NAPI C++ bridge layer. The ArkTS side is decoupled via the `IWriterCoreBridge` interface.
- `bindings`: Code for connecting the Rust core with native clients.

> Note: The previous Flutter client has been completely removed due to architectural conflicts.

For details, see the [Technical Route & Architecture Constraints](docs/TECHNICAL_ROUTE.md).

## Core Principles

- **Single Business Layer**: The Rust core (`core/writer_core`) is the sole entry point for file and data logic. Clients are not allowed to assemble paths or handle Workspace rules on their own.
- **Single Source of Truth**: `docs/workspace_format.md` defines the workspace format. It is maintained and operated by Rust Core.
- **Thin Clients**: Clients are only responsible for UI rendering, navigation, IME interaction, and theming. They should not contain any persistence or data logic.

## Development

### Tools

- `tools/build_core.sh`: Build the Rust core library.
- `scripts/generate_icons.py`: Generate app icon assets for each platform from `assets/brand/icon/source`.

### Rust Core

```bash
cd core/writer_core
cargo fmt
cargo check
cargo test
```

### Android Client

```bash
./tools/build_android.sh
```

**Supported targets**: Only `arm64-v8a` builds are officially supported. `x86_64` Android devices or emulators are not supported. To add `x86_64` support, open-source users can modify `tools/build_android.sh` and `apps/android/app/build.gradle.kts` to add the corresponding ABI.

### Linux Client

The Linux client uses Qt6 exclusively. CI, `apps/desktop/build.rs`, and the directory README all use Qt6 as the sole build pipeline; do not mix in Qt5 QML/plugin paths.

```bash
cargo run -p sujian-desktop
```

If you encounter rendering issues (such as double UI or black screen misalignment under Wayland), try running with basic render loop and enabling debug logs:

```bash
QSG_INFO=1 QSG_RENDER_LOOP=basic cargo run -p sujian-desktop
```

### HarmonyOS Client

Open the `apps/harmony/` directory in DevEco Studio. You need to build the prebuilt Rust FFI library first:

```bash
./tools/build_harmony.sh
```

Requirements:
- Add `aarch64-unknown-linux-ohos` target to your Rust toolchain
- Set the `OHOS_NDK_HOME` environment variable

### Manual Testing Steps

- Create a new project.
- Enter the project and confirm that "Volume 1" appears automatically.
- Click to create a new chapter and type some content.
- Confirm auto-save works.
- Exit and re-enter the chapter, confirm the content is still there.

## Documentation

- `docs/TECHNICAL_ROUTE.md`: Global technical route and architecture constraints
- `docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md`: Cross-platform capability contract and Core-first architecture constraints
- `docs/API_CONTRACTS.md`: Interface boundaries and interaction contracts (Bridge, Backend, QML)
- `docs/PRODUCT_DESIGN.md`: Product design and visual contract (product positioning, Monet colors, StarMap, settings design)
- `docs/workspace_format.md`: Single source of truth for document structure on disk
- `docs/settings_schema.md`: Local and syncable settings definitions
- `docs/sync_rules.md`: Sync rules
- `docs/starmap_semantics.md`: StarMap semantic foundation (independent objects and reference safety)
- `docs/editor_engine_route.md`: Custom editor engine and unified event layer route
- `docs/desktop_ime_notes.md`: Desktop (Linux) IME notes
