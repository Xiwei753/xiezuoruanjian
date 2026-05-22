# Rust Core 技术路线与实现边界

**本目录最高优先级规则**
- 本文档是 core/writer_core 目录的技术路线约束。
- 后续任何修改本目录的提示词、AI 任务、人工 PR，必须先读取本文档。
- 如果提示词和本文档冲突，以本文档为准。
- 如果确实需要改变路线，必须先提交本文档变更。

## 当前事实
- Rust Core 是跨平台业务真相来源。
- Workspace / Project / Volume / Chapter / Settings / Sync / Trash / Delete Guard / Mind Map 都应该由 Core 兜底。
- Android 和 Linux 不能绕过 Core 写长期业务数据。

## Core 职责
- 数据结构。
- 文件布局。
- 同步白名单。
- 删除安全。
- 导图业务图和布局快照。
- 错误语义。
- 序列化兼容。

## Core 禁止事项
- 不吞错误。
- 不返回假成功。
- 不允许 UI 层传错路径就删 workspace root。
- 不允许 Android/Linux 私自写 workspace 关键业务文件。
- 不把平台 UI 状态写入 Core 业务数据。

## Mind Map Core 路线
- MindMapGraph 是业务图。
- MindMapSnapshot 是渲染快照。
- MindMapAnchor / MindMapLink 绑定正文。
- Project/Volume/Chapter 结构图只是自动 fallback。
- 自定义 mind_map.json 必须先校验 project 存在。
- mind_map.json 损坏时必须有明确错误或明确 fallback 策略。

## 同步路线
- 同步白名单必须由 Core 管。
- 新增长期业务文件必须考虑同步白名单。
- token / secrets 不得进入同步文件。

## 序列化路线
- V1 JSON 可用。
- 大快照后续可升级 DirectByteBuffer / FlatBuffers / Protobuf。
- 升级时不能让平台 Renderer 直接依赖某种序列化细节。

## Core 路线变更规则
- 涉及文件布局、同步白名单、删除安全、跨平台模型的变更必须先看本文档。
- 如需改变核心数据格式，必须写兼容/迁移策略。
