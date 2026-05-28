# 架构总览

本仓库采用单仓库、多客户端、共享核心的架构。

> **注意：** 关于具体的技术约束和架构路线指南，请参阅 [技术路线与架构约束](TECHNICAL_ROUTE.md) 和 [跨平台能力契约与核心优先架构约束](CROSS_PLATFORM_CAPABILITY_CONTRACT.md)。这些根文档管理总体路线和跨平台能力对齐，而目录级文档管理具体的实现边界。

## 核心优先与能力契约

> **能力对齐状态：** 请查看 [跨平台能力矩阵](CAPABILITY_MATRIX.md) 了解 Rust Core、Android JNI 和 Linux 后端之间的当前对齐状态，以及将分支逻辑重构回核心的路线图。

为防止不同客户端（如 Android 和 Linux）之间的平台重复和状态分叉，本仓库强制执行**核心优先架构**：
- **单一事实来源**：`core/writer_core` 是业务逻辑、状态变更和验证规则的唯一所有者。
- **纯适配器**：Android JNI 层和 Linux Qt/QML 后端严格作为薄适配器，将 Core 数据包转换为 UI 视图。禁止实现独立的业务规则或直接修改工作区文件。
- **契约执行**：所有共享功能（工作区、项目、卷、章、设置、同步、思维导图、编辑器模型、AI）必须实现 [跨平台能力契约](CROSS_PLATFORM_CAPABILITY_CONTRACT.md) 中定义的标准能力 API 契约。

## 项目结构
- `core/writer_core`：用 Rust 编写的共享核心库。处理平台无关的逻辑、文档格式化、设置和同步规则。严格排除 UI、动画、输入法和窗口逻辑。（详见 [Rust Core 技术路线](../core/writer_core/TECHNICAL_ROUTE.md)）。
- `apps/android`：原生 Kotlin Android 客户端，目标是低功耗、稳定的输入法和一致的键盘交互。（详见 [Android 技术路线](../apps/android/TECHNICAL_ROUTE.md)）。
- `apps/linux`：原生 Linux 客户端，目标是使用 Qt/CMake 以获得与 X11/Wayland/fcitx5 的最佳集成。（详见 [Linux 技术路线](../apps/linux/TECHNICAL_ROUTE.md)）。
- `bindings`：连接 Rust 核心和原生客户端（Android 和 Linux）的接口代码。

所有客户端共享完全相同的工作区格式和同步规则。工作区的结构是文档格式的唯一事实来源。
