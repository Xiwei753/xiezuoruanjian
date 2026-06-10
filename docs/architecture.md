# 架构总览

本仓库采用单仓库、多客户端、共享核心的架构。

> **注意：** 关于具体的技术约束和架构路线指南，请参阅 [技术路线与架构约束](TECHNICAL_ROUTE.md) 和 [跨平台能力契约与核心优先架构约束](CROSS_PLATFORM_CAPABILITY_CONTRACT.md)。这些根文档管理总体路线和跨平台能力对齐，而目录级文档管理具体的实现边界。

## 核心优先与能力契约

> **能力对齐状态：** 请查看 [跨平台能力矩阵](CAPABILITY_MATRIX.md) 了解 Rust Core、Android UniFFI/legacy JNI 和 Desktop 后端之间的当前对齐状态，以及将分支逻辑重构回核心的路线图。

为防止不同客户端（如 Android 和 Desktop）之间的平台重复和状态分叉，本仓库强制执行**核心优先架构**：
- **单一事实来源**：`core/writer_core` 是业务逻辑、状态变更和验证规则的唯一所有者。
- **纯适配器**：Android 的 `AppServiceBridge + UniFFI` 主链路，以及 Desktop Qt6/QML 后端严格作为薄适配器，将 Core 数据包转换为 UI 视图。历史 JNI 符号仅是兼容实现细节，不是新的业务入口。禁止实现独立的业务规则或直接修改工作区文件。
- **契约执行**：所有共享功能（工作区、项目、卷、章、设置、同步、思维导图、编辑器模型、AI）必须实现 [跨平台能力契约](CROSS_PLATFORM_CAPABILITY_CONTRACT.md) 中定义的标准能力 API 契约。

## 项目结构
- `core/writer_core`：用 Rust 编写的共享核心库。处理平台无关的逻辑、文档格式化、设置和同步规则。严格排除 UI、动画、输入法和窗口逻辑。（详见 [Rust Core 技术路线](../core/writer_core/TECHNICAL_ROUTE.md)）。
- `apps/android`：原生 Kotlin Android 客户端，目标是低功耗、稳定的输入法和一致的键盘交互。（详见 [Android 技术路线](../apps/android/TECHNICAL_ROUTE.md)）。
- `apps/desktop`：原生 Rust + Qt6/QML Desktop 桌面客户端，通过 Cargo 构建并由 QObject 后端适配 Rust Core，目标是在 X11/Wayland/fcitx5 等 Desktop 操作系统下保持稳定集成。（详见 [Desktop 技术路线](../apps/desktop/TECHNICAL_ROUTE.md)）。
- `bindings`：连接 Rust 核心和原生客户端（Android 和 Desktop）的接口代码。

所有客户端共享完全相同的工作区格式和同步规则。工作区的结构是文档格式的唯一事实来源。
