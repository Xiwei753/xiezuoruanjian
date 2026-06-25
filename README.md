# 素笺写作

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Linux Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml)
[![Android Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml)
[![Windows Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/windows_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/windows_build.yml)

[English Version / 英文版](README_EN.md)

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

> **关于 Apple 平台**：目前没有适配 macOS / iOS 的打算。等什么时候搬砖凑够了 Apple Developer Program 的签名费用（688 元/年）再考虑适配。

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

如果遇到渲染问题（如 Wayland 下 UI 重影或黑屏错位），尝试使用基础渲染循环并启用调试日志：

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
- 进入作品，确认自动出现"第一卷"。
- 点击新建章节，并输入内容。
- 确认自动保存。
- 退出后重新进入该章节，确认内容仍在。

## 文档

- `docs/TECHNICAL_ROUTE.md`: 全局技术路线与架构约束
- `docs/archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md`: 跨平台能力契约与 Core-first 架构约束（已归档）
- `docs/archive/API_CONTRACTS.md`: 接口边界与交互契约（已归档）
- `docs/archive/PRODUCT_DESIGN.md`: 素笺产品设计与视觉契约（已归档）
- `docs/workspace_format.md`: 磁盘上文档结构的单一事实来源
- `docs/settings_schema.md`: 本地和可同步设置定义
- `docs/sync_rules.md`: 同步规则
- `docs/starmap_semantics.md`: 星图语义地基 (独立对象与引用安全)
- `docs/starmap_canvas_model.md`: 星图画布模型契约
- `docs/starmap_implementation_route.md`: 星图实现路线
- `docs/editor_engine_route.md`: 自绘编辑器与统一事件层路线
- `docs/harmony_route.md`: 鸿蒙端技术路线
- `docs/harmony_bridge_contract.md`: 鸿蒙桥接契约
- `docs/harmony_core_action_map.md`: 鸿蒙 Core Action 映射
- `docs/harmony_native_bridge_plan.md`: 鸿蒙 Native Bridge 接入计划
- `docs/archive/desktop_ime_notes.md`: Desktop (Linux) 输入法笔记（已归档）
- `docs/archive/screen_policy_acceptance.md`: 三端布局验收表（已归档）
