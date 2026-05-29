# 写作应用 (多客户端架构)

本仓库包含一个跨平台写作应用的源代码，目前已过渡到“Rust 核心 + 各平台轻量客户端”的单仓库架构。

## 架构

- `core/writer_core`: 使用 Rust 编写的**唯一**业务底层核心库。处理所有文件 I/O、项目管理、同步、格式化和设置规则。严格排除 UI 逻辑。
- `apps/android`: 原生 Kotlin Android 客户端。作为当前的主客户端。主业务入口为 `AppServiceBridge + UniFFI`，领域 Bridge/Repository/ViewModel/UI 只做平台适配；`NativeCoreBridge` 仅保留为 legacy JSON/JNI 兼容 fallback。
- `apps/linux`: 原生 Linux 客户端站位（骨架），未来使用 Qt/C++。同样必须通过 bindings 调用 Rust 核心。
- `bindings`: 用于连接 Rust 核心和原生客户端的代码。

> 注：原有的 Flutter 客户端由于其架构冲突已彻底移除。有关历史信息，可参阅 `docs/legacy/flutter_removed.md`。

详情请参阅 `docs/architecture.md`。

## 核心原则

- **唯一业务底层**：Rust 核心 (`core/writer_core`) 是文件和数据逻辑的唯一入口。客户端不允许自行拼接路径、处理 Workspace 规则。
- **单一事实来源**：`docs/workspace_format.md` 定义了工作区格式。它由 Rust Core 维护和操作。
- **客户端做薄**：客户端仅负责 UI 渲染、导航、输入法交互和主题。不应包含任何持久化和数据逻辑。

## 开发

### 工具

- `tools/build_core.sh`: 构建 Rust 核心库。

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

由于尚未实现完整的构建脚本，可以使用以下命令运行：

```bash
cargo run -p linux
```

如果遇到渲染相关问题（例如 Wayland 下的双重 UI 或黑屏错位），可尝试使用以下命令开启基础渲染循环并打开调试日志：

```bash
QSG_INFO=1 QSG_RENDER_LOOP=basic cargo run -p linux
```

### 实机测试验证步骤

- 新建作品。
- 进入作品，确认自动出现“第一卷”。
- 点击新建章节，并输入内容。
- 确认自动保存。
- 退出后重新进入该章节，确认内容仍在。

## 文档

- `docs/architecture.md`: 整体架构
- `docs/workspace_format.md`: 磁盘上文档结构的单一事实来源。
- `docs/settings_schema.md`: 本地和可同步设置定义。
- `docs/core_api.md`: Rust 核心库 API。
- `docs/sync_rules.md`: 同步规则。
- `docs/client_feature_matrix.md`: 客户端功能矩阵。
- `docs/ai_development_guide.md`: 针对 AI 辅助开发的重要说明。
- `docs/legacy/flutter_removed.md`: 遗留 Flutter 客户端移除说明。
