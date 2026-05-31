# 跨平台能力矩阵

> 本矩阵旨在澄清 Core API 与各端 Adapter 的接入状态。
> **注意**：Android 当前的主业务入口为 `AppServiceBridge + UniFFI + WriterCoreApi`。旧的 `NativeCoreBridge` 等 JNI 入口被视为 legacy 残留，不得作为新业务实现路线。Linux 正在向 `WriterCoreApi` 迁移，剩余未迁移的能力（如星图编辑）标记为 legacy facade。

本文档是迁移现有代码到 Core-first Capability Contract 的盘点表。
本文档不定义新路线，只记录当前事实和待收敛项。
后续每次新增业务能力，都必须更新本文档。

## 1. WorkspaceCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 创建/打开工作区 | `validate_workspace` | `WriterCoreApi::create_workspace`, `validate_workspace` | `WorkspaceBridge -> AppServiceBridge` | `NativeCoreBridge`, 旧 JSON/JNI fallback | `WriterCoreApi` | 无 | 否 | P1 | Linux 已移除直接文件系统判断，改为全权调用 Core API，需统一封装 ResultEnvelope |

**小结**：
- Linux backend 已修复直接文件 I/O，目前通过 API 与 Core 对接。
- Android 已有 `BridgeResult` / JNI fallback 的 `ResultEnvelope` 基础包装，但 UniFFI Capability 仍未全量返回 envelope。
- 需要继续统一使用 ResultEnvelope 返回验证和创建结果。

## 2. ProjectCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 创建作品 | `create_project` | `create_project` | `AppServiceBridge + UniFFI` | `NativeCoreBridge.createProject` | `WriterCoreApi` | 无 | 否 | P0 | 已修 Linux 新建作品（废除文件写入，依赖 API）。需统一 ResultEnvelope |
| 获取作品列表 | `list_projects` | `list_projects` | `AppServiceBridge + UniFFI` | `NativeCoreBridge.listProjects` | `WriterCoreApi` | 无 | 否 | P1 | 统一 List 接口，支持按需获取 |
| 修改/删除 | `rename_project`, `delete_project`, `reorder_projects` | `rename_project`, `delete_project`, `reorder_projects` | `AppServiceBridge + UniFFI` | `NativeCoreBridge.renameProject`... | `WriterCoreApi` | 无 | 否 | P1 | 统一返回 ResultEnvelope |

**小结**：
- Linux 新建作品已收敛，完全依赖 Core `create_project`。
- Android 仍在部分依赖 Legacy Bridge；legacy JSON 已开始返回标准 ResultEnvelope 字段。
- 必须继续把 UniFFI 与 Linux backend 的散落返回收敛到 ResultEnvelope。

## 3. VolumeCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 增删改查 | `create_volume`... | `create_volume`... | `AppServiceBridge + UniFFI` | `NativeCoreBridge.createVolume`... | `WriterCoreApi` | 无 | 否 | P1 | 统一返回标准 ResultEnvelope，废除特供接口 |

**小结**：
- 增删改查基本都存在于 Core，但平台层各自用不同方式消化了返回数据。
- 存在 raw string error 抛给 UI 层的情况。

## 4. ChapterCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 创建与编辑 | `create_chapter`, `read_chapter`, `write_chapter` | `create_chapter`, `open_chapter`, `save_chapter_content` | `AppServiceBridge + UniFFI` | `NativeCoreBridge.createChapter`... | `WriterCoreApi` | `writing_bridge` 的少量事件记录 | 否 | P1 | 状态需从 Core 事件获得，而不是 UI/Backend 自行维护 |
| 属性与删除 | `list_chapters`... | `list_chapters`... | `AppServiceBridge + UniFFI` | `NativeCoreBridge` 相关接口 | `WriterCoreApi` | 无 | 否 | P1 | 统一定义 |

**小结**：
- Linux 的 `writing_bridge` 已迁移至 `WriterCoreApi` 作为主入口。
- 不存在直接写文件，但存在独立的状态维护（如 `current_save_status`）。

## 5. SettingsCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 本地设置 | `load_local_settings`, `save_local_settings` | `load_local_settings`, `save_local_settings`, `save_local_settings_envelope_json` | `SettingsBridge -> AppServiceBridge` | 待盘点 | `WriterCoreApi` | 无 | 部分 | P0 | Android 已改为消费 Core envelope 的 `SettingsSaved`；Linux 仍需接入同一事件模型 |
| 同步设置 | `load_syncable_settings`, `save_syncable_settings` | `load_syncable_settings`, `save_syncable_settings`, `save_syncable_settings_envelope_json` | `SettingsBridge -> AppServiceBridge` | 待盘点 | `WriterCoreApi` | 无 | 部分 | P1 | Android 已改为消费 Core envelope 的 `SettingsSaved`；Linux 仍需补齐监听链路 |

**小结**：
- Android 已移除 `SettingsChangeBus.notifyChanged()` / `markChanged()` 这类平台自行发事件路径，改为由 `CoreSettingsEvents` 记录 Core envelope 中的 `SettingsSaved`。
- Core API 已提供设置保存 envelope JSON，并在 `changedEntities` 标记 `SettingsSaved`；Linux 仍需接入同一事件消费链路。

## 6. SyncCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 同步执行 | `sync_workspace`, `perform_sync`... | `perform_sync`, `perform_sync_dry_run` | `AppServiceBridge + UniFFI` | 待盘点 | `WriterCoreApi` | 无 | 否 | P0 | 在 Core 映射 libgit2 checkout conflict；双端依赖 Core 返回统一的 `ResultEnvelope` |
| 同步配置 | `load_sync_config`, `save_sync_config` | `load_sync_config`, `save_sync_config` | `AppServiceBridge + UniFFI` | 待盘点 | `WriterCoreApi` | 无 | 否 | P1 | 配置读取应完全一致 |

**小结**：
- Linux GitHub 同步 Conflict 时自己判断冲突。
- 必须立刻统一 SyncCapability 的 ResultEnvelope 及 conflict 状态收敛。

## 7. MindMapCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 图谱与编辑 | `create_mind_map_graph` 等 | 待盘点 | 待盘点 | `NativeCoreBridge` | 待盘点 | `facade::WriterCore` (TODO(api)) | 否 | P1 | Linux backend 需要补齐同等的 MindMap Capability 入口 |
| 渲染视图 | `get_mind_map_snapshot` | 待盘点 | 待盘点 | `NativeCoreBridge` | 待盘点 | `facade::WriterCore` (TODO(api)) | 否 | P1 | 补齐 |

**小结**：
- 能力主要在 Android 的 Legacy 桥接存在，Linux 端存在严重缺失。

## 8. EditorModelCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 统计/模型 | `record_recent_edit` | `record_recent_edit` | `AppServiceBridge + UniFFI` | `NativeCoreBridge` | 待盘点 | `facade::WriterCore` (TODO(api)) | 否 | P2 | 统一走 EditorModelCapability 规范 |

**小结**：
- Editor 动画属于平台渲染逻辑，其启用状态未有效挂载到 Core 的 Settings 统一入口。

## 9. Trash/DeleteGuardCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 软删除 | `move_chapter_to_trash` | 待盘点 | 待盘点 | 待盘点 | 待盘点 | 待盘点 | 否 | P1 | 彻底盘点并强制应用 Delete Guard |

## 10. SearchCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 搜索 | 缺失 | 缺失 | 待盘点 | 本地视图搜索 | 缺失 | 缺失 | 否 | P1 | 将搜索算法收拢到 Core 层 |

## 11. AICapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| AI 功能上下文 | `build_ai_context` | 待盘点 | 缺失 | 缺失 | 缺失 | 缺失 | 否 | P1 | 统一设计 AI Event/Command |

---

## 下一步重构顺序 (Refactoring Roadmap)

1. **P0-2：SyncCapability ResultEnvelope + conflict 状态收敛**
   - 统一所有的 Core Sync API 响应为标准 `ResultEnvelope`。
   - 明确在 Core 中映射 libgit2 的 checkout conflict。
   - Android 同步和 Linux 同步统一消费 `SyncStatus.conflict`，停止使用裸字符串 (rawError) 或自己封装的 `SyncTaskOutcome`。
2. **P0-3：SettingsCapability 保存/事件收敛，替代平台临时 bus**
   - Android 端已废除 `SettingsChangeBus.notifyChanged()` / `markChanged()`，保存设置后只消费 Core envelope 的 `SettingsSaved`。
   - 继续补齐 Linux 侧 Core `SettingsSaved` 事件消费链路。
   - 双端保存设置统一经由 `saveLocalSettings` 等接口，UI 被动响应 Core 事件完成字号和动画开关刷新。
3. **P1-1：MindMapCapability 双端入口补齐**
   - Linux backend 需要按 Android `NativeCoreBridge` 的已有模式，对接并暴露完整的 MindMap Edit 和 Snapshot API，确保双端能力对称。
4. **P1-2：统一 ResultEnvelope 类型**
   - `api::ResultEnvelope`、Android `BridgeResult` 包装和 JNI fallback 序列化基础层已落地。
   - 继续在 UniFFI 与 Linux Backend 的所有接口中，用标准化的 JSON/DTO 结构体替代现有各种散落的特化 JSON 拼接。
5. **P1-3：Android JNI / Linux backend 薄适配器化**
   - 清除所有的业务判断（如 `workspace_path.is_empty()` 判断、权限判定提示），全权由 Core 拒绝并返回错误码。
