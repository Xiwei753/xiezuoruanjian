# 写作应用 (多客户端架构)

本仓库包含一个跨平台写作应用的源代码，目前正在过渡到单仓库、多客户端、共享核心的架构。

## 架构

- `core/writer_core`: 使用 Rust 编写的共享核心库。处理平台无关逻辑、文档格式化、设置和同步规则。严格排除 UI、动画、输入法和窗口逻辑。
- `apps/flutter_legacy`: 现有的 Flutter 客户端，保留作为遗留原型和参考。
- `apps/android_native`: 原生 Kotlin Android 客户端（骨架）。目标是低功耗、稳定的输入法和一致的键盘交互。
- `apps/linux_native`: 原生 Linux 客户端（骨架），使用 Qt/CMake 目标是最佳集成 X11/Wayland/fcitx5。
- `bindings`: 用于连接 Rust 核心和原生客户端的代码。

详情请参阅 `docs/architecture.md`。

## 核心原则

- **单一事实来源**：`docs/workspace_format.md` 定义了确切的目录结构和文件格式。所有客户端必须遵守。UI 层不能私自更改格式。
- **核心平台无关**：Rust 核心 (`core/writer_core`) 仅处理文件 I/O、数据逻辑和平台无关的格式化。
- **统一同步规则**：所有客户端共享相同的同步和冲突解决规则 (`docs/sync_rules.md`)。

## 开发

### 工具

- `tools/validate_workspace.py`: 根据 `docs/workspace_format.md` 规范验证工作区的结构。
- `tools/check_all.sh`: 运行格式化、代码检查、测试和夹具验证。
- `tools/build_core.sh`: 构建 Rust 核心库。
- `tools/build_android_native.sh`: 构建 Android 客户端的占位符。
- `tools/build_linux_native.sh`: 构建 Linux 客户端骨架。
- `tools/build_flutter_legacy.sh`: 构建遗留的 Flutter 客户端。

### Rust 核心

```bash
cd core/writer_core
cargo build
cargo test
```

### 遗留 Flutter 客户端

运行遗留的 Flutter 应用程序：

```bash
cd apps/flutter_legacy
flutter pub get
# 标准运行命令可能仍可用于测试遗留逻辑
flutter run
```

*注意：Flutter 客户端不再是新功能开发的主要焦点。其结构仅作为原型参考保留。*

## 文档

- `docs/architecture.md`: 整体架构
- `docs/workspace_format.md`: 磁盘上文档结构的单一事实来源。
- `docs/settings_schema.md`: 本地和可同步设置定义。
- `docs/core_api.md`: Rust 核心库 API。
- `docs/sync_rules.md`: 同步规则。
- `docs/client_feature_matrix.md`: 客户端功能矩阵。
- `docs/ai_development_guide.md`: 针对 AI 辅助开发的重要说明。
