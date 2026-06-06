# Rust Core 技术路线与实现边界

**本目录最高优先级规则**
- 本文档是 core/writer_core 目录的技术路线约束。
- 后续任何修改本目录的提示词、AI 任务、人工 PR，必须先读取本文档。
- 如果提示词和本文档冲突，以本文档为准。
- 如果确实需要改变路线，必须先提交本文档变更。

## 当前事实与全局契约
- **跨平台唯一业务入口：** 所有跨平台业务能力统一由 Core Capability API 定义和暴露，详情参考全局 [《跨平台能力契约与 Core-first 架构约束》](../../docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md)。
- Rust Core 是所有客户端唯一的业务真相来源。
- Workspace / Project / Volume / Chapter / Settings / Sync / Trash / Delete Guard / Mind Map 都由 Core 统一兜底与定义。
- Android 和 Linux 等所有端均不能绕过 Core 提供的 Capability API 读写长期业务数据，也不得在平台端自行编写重复的业务逻辑。
- `src/api/` 是当前跨平台暴露层底座：`types.rs` 放稳定 DTO，`error.rs` 放平台稳定错误映射，`service.rs` 放 `WriterCoreApi` 服务。
- `src/app_service.rs` 只作为 UniFFI adapter 保留 `WriterAppService` 兼容入口；不得再作为 DTO、错误映射和业务 API 的混杂事实来源。
- `src/api.udl` 是 UniFFI 绑定声明，不是业务 API 设计的唯一事实来源。

## Core 职责
- 数据结构。
- 文件布局。
- 同步白名单。
- 删除安全。
- 导图业务图和布局快照。
- 编辑器平台无关事务、选区、变更和动画事件语义。
- 错误语义。
- 序列化兼容。
- 跨平台稳定 API DTO、错误映射和服务入口。

## Core 禁止事项
- 不吞错误。
- 不返回假成功。
- 不允许 UI 层传错路径就删 workspace root。
- 不允许 Android/Linux 私自写 workspace 关键业务文件。
- 不把平台 UI 状态写入 Core 业务数据。
- 不把业务能力直接堆进 UniFFI adapter。
- 不包含平台绘制、输入法、窗口、动画曲线执行或 Qt/Android 特定代码。

## Editor Core 路线
- `src/editor` 是编辑器平台无关语义层。
- `EditorTransaction` 统一描述旧文本、新文本、变更列表、选区变化和原因。
- `EditorAnimationEvent` 只描述 renderer 可以播放的插入、删除、光标事件，不执行绘制。
- Platform renderer 可以使用 Qt `QTextLayout`、Android `Canvas` / text layout 等本地能力，但不得在平台端自行定义分叉的业务 diff 语义。

## Mind Map Core 路线
- **分层边界**：必须严格区分 Graph（业务真相）、Layout（纯位置信息）和 Snapshot（发给平台的只读渲染视图）。
- **存储结构 (V2)**：使用 `projects/<id>/mind_map/graphs/`、`layouts/` 等模块化目录，弃用 V1 单文件方案。引入 `projects/<id>/mind_map/index.json` 作为图的索引，包含 `schemaVersion` (必须为 2)、`defaultGraphId`、`graphIds` 和 `updatedAt`。如果多图存在但无索引文件，则必须返回明确错误。
- **Schema 与迁移**：目前 schemaVersion 为 2。支持从 V1 `mind_map.json` 解析并自动填充新节点字段，遇到不支持的 schemaVersion 必须返回明确错误。
- **Anchor 解析规则**：优先使用 exact offset。漂移时，使用 `prefix + selectedText + suffix` 重新定位。未找到时明确返回 `BrokenAnchor`，不在拖拽时运行该操作。
- MindMapGraph 是业务图。
- MindMapSnapshot 是渲染快照。
- MindMapAnchor / MindMapLink 绑定正文。
- Project/Volume/Chapter 结构图只是自动 fallback。
- 自定义 mind_map.json 必须先校验 project 存在。
- 导图损坏时必须有明确错误，不能静默降级为自动章节图。

## 同步路线
- 同步白名单必须由 Core 管。
- 新增长期业务文件（如 `projects/<id>/mind_map/` 下的文件）必须考虑同步白名单。
- token / secrets 不得进入同步文件。

## 序列化路线
- V1 JSON 可用。
- 大快照后续可升级 DirectByteBuffer / FlatBuffers / Protobuf。
- 升级时不能让平台 Renderer 直接依赖某种序列化细节。

## Core 路线变更规则
- 涉及文件布局、同步白名单、删除安全、跨平台模型的变更必须先看本文档。
- 如需改变核心数据格式，必须写兼容/迁移策略。
