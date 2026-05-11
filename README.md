# 写作软件 (Writer App) - 多客户端架构 (Multi-Client Architecture)

本项目是一个跨平台写作软件的源代码仓库。目前已重构为“单仓库、多原生客户端、共享核心”的架构。

本说明仅供开发者参考，不对软件的运行产生任何影响。

## 目录结构及架构 (Architecture)

- `core/writer_core`: 共享的 Rust 核心库。负责所有的平台无关逻辑，例如工作区读写、配置存储、同步规则和缓存管理。**不包含任何 UI、动画、输入法或窗口逻辑。**
- `apps/flutter_legacy`: 原有的 Flutter 客户端。现已被降级为历史代码原型 (Legacy/Prototype)，不再是未来开发的重点，但代码已完整保留以供参考。
- `apps/android_native`: Android Kotlin 原生客户端的**骨架 (Skeleton)**。未来将目标定为追求低发热、输入法稳定和流畅的原生体验。
- `apps/linux_native`: Linux C++/Qt 原生客户端的**骨架 (Skeleton)**。未来将目标定为提供在 X11/Wayland/fcitx5 等环境下的最佳原生体验。
- `bindings`: 存放连接 Rust 核心库和原生客户端绑定代码的位置（如 JNI, FFI）。

更多详情请参考 `docs/architecture.md`。

## 核心原则 (Core Principles)

- **唯一的文档真相源 (Single Source of Truth)**: 所有的原生应用必须严格遵守 `docs/workspace_format.md` 定义的工作区存储格式规范。UI 层严禁私自引入和使用不同的文件格式。
- **纯粹的 Rust 核心库**: `core/writer_core` 仅负责底层逻辑。绝不允许混入任何具体平台的调用（如 Flutter 的 API、Qt 的窗口代码）。
- **统一的同步机制**: 所有客户端共享同一套冲突和同步方案 `docs/sync_rules.md`。

## 如何运行与检查 (Development Tools)

我们在仓库根目录下的 `tools/` 目录提供了相关的集成脚本：

- `tools/validate_workspace.py`: 用于校验工作区数据文件结构和 JSON Schema 是否符合格式规范。
- `tools/check_all.sh`: 运行所有安全检查和构建（包含 Rust 库的格式校验、编译、测试，以及测试验证和 Linux Skeleton 构建验证）。
- `tools/build_core.sh`: 构建 Rust 核心库。
- `tools/build_linux_native.sh`: 构建 Linux 的 C++ Native 原型骨架。
- `tools/build_flutter_legacy.sh`: 运行原有 Flutter 原型的基本包获取构建脚本。

### 运行 Rust 核心库检查

```bash
cd core/writer_core
cargo build
cargo test
```

### 运行旧版 Flutter 客户端

如果您依然需要测试旧版 Flutter 的功能（如平滑滚动、输入动画等），可以使用以下方式执行：

```bash
cd apps/flutter_legacy
flutter pub get
flutter run
```

## 文档指引 (Documentation Reference)

- `docs/architecture.md`: 整体多端架构说明。
- `docs/workspace_format.md`: **最重要的文件格式说明**，定义了文档目录树结构的唯一的真相源。
- `docs/settings_schema.md`: 区分定义了 `local` 和 `sync` 配置的内容规范。
- `docs/core_api.md`: Rust `writer_core` 的 API 设计文档。
- `docs/sync_rules.md`: 同步和冲突合并规则。
- `docs/client_feature_matrix.md`: 展示了遗留版本 Flutter 客户端以及 Android / Linux 原生骨架的功能进度对比。
- `docs/ai_development_guide.md`: 后续使用 AI 协助开发时的边界与安全注意事项。

---

*历史说明: 本项目先前已完成有关 Flutter UI 稳定性相关的改进（例如防抖保存、平滑滚动、输入动画安全和光标定位的健壮性），相关代码与测试均已迁移至 `apps/flutter_legacy` 目录下。*
