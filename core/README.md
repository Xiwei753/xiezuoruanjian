# Core 核心层

本目录包含了项目的共享核心业务逻辑。遵循 **"Rust Core is the single source of truth" (Rust 核心是唯一的业务事实来源)** 架构原则。

## 目录结构说明

- [writer_core/](file:///home/xiwei/xiezuoruanjian/core/writer_core/)：核心逻辑 Rust 库。负责所有的文件 I/O、项目管理、同步策略、格式化规则和用户设置，完全不依赖任何 UI 框架。

## 核心职责

1. **唯一真相来源**：所有涉及工作区格式（Workspace Format）、数据持久化、版本管理、以及冲突解决的核心业务逻辑，必须在这一层实现。
2. **跨平台重用**：通过 `bindings/` 目录的桥接层，同时为 Android（Kotlin）和 Linux（Qt/QML）客户端提供统一的底层服务。
3. **平台中立**：严格排除任何平台特定的 UI 逻辑、窗口管理、动画或输入法代码。
