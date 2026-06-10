# 跨平台能力矩阵

> 本矩阵旨在澄清 Core API 与各端 Adapter 的接入状态。
> **注意**：Android 当前的主业务入口为 `AppServiceBridge + UniFFI + WriterCoreApi`。旧的 `NativeCoreBridge` 等 JNI 入口被视为 legacy fallback，不得作为新业务实现路线。Linux 正在向 `WriterCoreApi` 迁移，剩余未迁移的能力（如星图编辑）标记为 legacy facade。

本文档是迁移现有代码到 Core-first Capability Contract 的盘点表。
本文档不定义新路线，只记录当前事实和待收敛项。
后续每次新增业务能力，都必须更新本文档。

当前口径：写入、删除、设置保存、同步执行/保存等高风险操作必须消费 Core `ResultEnvelope`，并通过 `errorCode` / `userMessage` / `changedEntities` 驱动平台层状态；只读列表、打开、统计等 typed DTO 入口可保留 typed direct call。

## 1. WorkspaceCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 创建/打开工作区 | `validate_workspace` | `create_workspace`, `validate_workspace`, `get_workspace_diagnostics_envelope_json` | `WorkspaceBridge -> AppServiceBridge` | `NativeCoreBridge` fallback | `WriterCoreApi` | 无 | 诊断已对齐 | P1 | 继续缩小 Android legacy fallback；只读 typed DTO 不强制改 envelope |

**小结**：
- Linux 打开/恢复工作区的直接文件 I/O 已移除；工作区诊断由 Core 探测 manifest、projects、app-meta、可写性和 `createProjectAvailable`，Linux/QML 只展示 envelope 数据。
- Android 已具备 `BridgeResult` / `ResultEnvelope` 解析层；Workspace 读路径仍保留 typed DTO 或 legacy fallback，不作为高风险写操作口径。

## 2. ProjectCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 创建作品 | `create_project` | `create_project`, `create_project_envelope_json` | `WorkspaceBridge/AppServiceBridge` 消费 envelope JSON | `NativeCoreBridge.createProject` fallback | `WriterCoreApi` typed | 无 | Android 主写路径已对齐 | P1 | Linux 如需事件联动，应消费同一 `ProjectCreated` 标记 |
| 获取作品列表 | `list_projects` | `list_projects` | `AppServiceBridge + UniFFI` typed DTO | `NativeCoreBridge.listProjects` fallback | `WriterCoreApi` typed | 无 | 读路径已收敛到 Core | P2 | 保持只读 DTO，避免平台自行遍历磁盘 |
| 修改/删除/排序 | `rename_project`, `delete_project`, `reorder_projects` | `rename_project_envelope_json`, `delete_project_envelope_json`, `reorder_projects_envelope_json` | `AppServiceBridge` 消费 envelope JSON | `NativeCoreBridge.renameProject` 等 fallback | `WriterCoreApi` typed | 无 | Android 主写路径已对齐 | P1 | 删除已进入 Core 软删除/墓碑链路；后续补 Linux 事件消费 |

**小结**：
- Android Project 创建、重命名、删除、排序已从 typed direct call 切到 Core envelope JSON。
- Core envelope 写入 `ProjectCreated`、`ProjectRenamed`、`ProjectDeleted`、`ProjectsReordered` 到 `changedEntities`，平台层不得自行发业务事件替代该标记。

## 3. VolumeCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 增删改排序 | `create_volume`, `rename_volume`, `delete_volume`, `reorder_volumes` | `create_volume_envelope_json`, `rename_volume_envelope_json`, `delete_volume_envelope_json`, `reorder_volumes_envelope_json` | `AppServiceBridge` 消费 envelope JSON | `NativeCoreBridge.createVolume` 等 fallback | `WriterCoreApi` typed | 无 | Android 主写路径已对齐 | P1 | Linux 如需状态刷新，应接入 `VolumeCreated/VolumeRenamed/VolumeDeleted/VolumesReordered` |
| 查询卷列表 | `list_volumes` | `list_volumes` | `AppServiceBridge + UniFFI` typed DTO | `NativeCoreBridge.listVolumes` fallback | `WriterCoreApi` typed | 无 | 读路径已收敛到 Core | P2 | 保持只读 DTO |

**小结**：
- Android Volume 高风险写操作已统一走 Core envelope，错误分支由 `errorCode` / `userMessage` 驱动。
- Core 删除卷会走 delete guard、trash 和 tombstone 基础链路；完整回收站能力另见 Trash/DeleteGuardCapability。

## 4. ChapterCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 创建/重命名/排序 | `create_chapter`, `rename_chapter`, `reorder_chapters` | `create_chapter_envelope_json`, `rename_chapter_envelope_json`, `reorder_chapters_envelope_json` | `WritingBridge/AppServiceBridge` 消费 envelope JSON | `NativeCoreBridge.createChapter` 等 fallback | `WriterCoreApi` typed | `writing_bridge` 少量 UI 状态 | Android 主写路径已对齐 | P1 | Linux 状态刷新应由 Core 标记驱动 |
| 打开/读取 | `read_chapter`, `open_chapter` | `open_chapter`, `list_chapters` | `AppServiceBridge + UniFFI` typed DTO | `NativeCoreBridge` fallback | `WriterCoreApi` typed | 无 | 读路径已收敛到 Core | P2 | 保持 `ChapterOpenResult` 为唯一权威返回 |
| 保存/清空/备注/删除 | `write_chapter`, `clear_chapter_content`, `update_chapter_note`, `delete_chapter` | `save_chapter_content_envelope_json`, `clear_chapter_content_envelope_json`, `update_chapter_note_envelope_json`, `delete_chapter_envelope_json` | `WritingBridge/AppServiceBridge` 消费 envelope JSON | `NativeCoreBridge` fallback | `WriterCoreApi` typed | `writing_bridge` 少量 UI 状态 | Android 主写路径已对齐 | P1 | 自动保存、清空和删除均不得绕过 Core envelope |

**小结**：
- Android Chapter 创建、重命名、排序、保存、清空、备注、删除已统一消费 Core envelope。
- Core envelope 写入 `ChapterCreated`、`ChapterRenamed`、`ChaptersReordered`、`ChapterSaved`、`ChapterCleared`、`ChapterNoteUpdated`、`ChapterDeleted` 到 `changedEntities`。
- 正文文件仍永远是纯文本；排版和 UI 状态不得改变 Core 保存内容。

## 5. SettingsCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 本地设置 | `load_local_settings`, `save_local_settings` | `load_local_settings`, `save_local_settings_envelope_json` | `SettingsBridge -> AppServiceBridge` 消费 envelope JSON | 无主路径 | `WriterCoreApi` typed | 无 | Android 保存事件已对齐 | P1 | Linux 如需即时 UI 刷新，应消费 `SettingsSaved` |
| 同步设置 | `load_syncable_settings`, `save_syncable_settings` | `load_syncable_settings`, `save_syncable_settings_envelope_json` | `SettingsBridge -> AppServiceBridge` 消费 envelope JSON | 无主路径 | `WriterCoreApi` typed | 无 | Android 保存事件已对齐 | P1 | Linux 补齐同一事件消费链路 |

**小结**：
- Android 已移除 `SettingsChangeBus.notifyChanged()` / `markChanged()` 这类平台自行发事件路径，改为消费 Core envelope 中的 `SettingsSaved`。
- 设置读取仍是 typed DTO；保存必须走 envelope。

## 6. SyncCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 同步执行 | `sync_workspace`, `perform_sync` | `perform_sync_envelope_json`, `perform_sync_dry_run_envelope_json`, `perform_sync_diagnostics_envelope_json` | `SyncBridge -> AppServiceBridge` 消费 envelope JSON | 无主路径 | `WriterCoreApi` / `sync_bridge` | 无 | Android 主执行路径已对齐 | P1 | Linux 同步 UI 继续减少自定义 outcome/字符串分支 |
| 同步配置/密钥保存 | `load_sync_config`, `save_sync_config`, `save_sync_secrets` | `save_sync_config_envelope_json`, `save_sync_secrets_envelope_json` | `SyncBridge -> AppServiceBridge` 消费 envelope JSON | 无主路径 | `WriterCoreApi` | 无 | Android 保存路径已对齐 | P1 | 读取保留 typed DTO；保存统一读取 `SyncConfigSaved/SyncSecretsSaved` |

**小结**：
- Android `saveSyncConfig`、`saveSyncSecrets`、`performSync`、`performSyncDryRun`、`performSyncDiagnostics` 已全部改为消费 Core sync envelope。
- Core 在同步 envelope 中统一映射 `SYNC_CONFLICT`、`SYNC_AUTH_REQUIRED`、`SYNC_NETWORK_ERROR`、`SYNC_REMOTE_ERROR`、`SYNC_CONFIG_ERROR`、`SYNC_NOT_CONFIGURED`、`SYNC_FAILED` 等错误码。
- 当前 Android `BridgeResult.Error` 会保留 envelope 的 `errorCode` / `userMessage` / `changedEntities`，但不会把失败 envelope 中的 `data` 作为成功数据分发；冲突详情 UI 如需结构化数据，需要单独扩展错误分支数据承载。

## 7. MindMapCapability (LEGACY - 已废弃)

> **⚠️ 正式图谱路线为 StarMap。** MindMap 仅保留用于旧数据迁移兼容，禁止新增功能。

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 图谱快照 | `get_mind_map_snapshot` | `get_mindmap_snapshot`, `get_mindmap_snapshot_json` | `MindMapBridge -> AppServiceBridge` typed DTO | 旧 JNI fallback | 待迁移到 `WriterCoreApi` | `facade::WriterCore` legacy | 快照读路径有 Core 入口 | P1 | Linux 补齐同等 snapshot 入口 |
| 图谱编辑/保存 | `save_mindmap_graph`, edit graph methods | `save_mindmap_graph` 及强类型 DTO edit API | 尚未形成 Android 主编辑路径 | 旧 JNI fallback | legacy facade | `facade::WriterCore` legacy | 未产品化 | P1 | 双端统一走强类型 DTO；后续再决定是否需要 envelope 写入口 |

**小结**：
- MindMap 已有 Core snapshot / graph DTO 能力，Android 目前只接入 snapshot 读路径。
- 图谱编辑和保存尚未形成双端一致的产品化入口，不能宣称完整收口。

## 8. EditorModelCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 统计/模型 | `record_recent_edit`, `writing_stats` | `record_recent_edit`, `record_writing_event`, `flush_writing_stats` 等 | `AppServiceBridge + UniFFI` typed DTO | `NativeCoreBridge` fallback | 待补齐主入口说明 | `facade::WriterCore` legacy | 统计写入在 Core | P2 | 后续统一事件展示模型 |

**小结**：
- Editor 动画属于平台渲染逻辑；是否启用应通过 Settings 统一入口持久化。
- 写作统计已在 Core，但跨端展示模型仍需继续收敛。

## 9. Trash/DeleteGuardCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 删除防护与软删除基础 | `delete_guard`, project/volume/chapter delete, `move_chapter_to_trash` | `delete_project_envelope_json`, `delete_volume_envelope_json`, `delete_chapter_envelope_json`, `move_chapter_to_trash` | Project/Volume/Chapter 删除经 `AppServiceBridge` envelope JSON | 旧 JNI fallback | `WriterCoreApi` typed | 无 | 基础删除链路已收口 | P1 | 补完整回收站：列表、恢复、永久删除、清理策略 |

**小结**：
- 当前只能说“删除不再裸奔”：Core 删除会经过 delete guard，移动到 `app-meta/sync/trash/` 并记录 tombstone，Android 主删除路径已消费 envelope。
- 这还不是完整回收站系统；没有统一的 trash list / restore / purge / retention API，UI 不得宣称完整回收站能力。

## 10. SearchCapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| 编辑器内搜索 | 缺失 | 缺失 | `EditorActivity` 当前正文内本地搜索/替换 | 无主路径 | 平台本地搜索 | 无 | 平台本地能力 | P2 | 若要跨章节/跨项目搜索，先定义 Core Search API |

**小结**：
- Search 当前不是 Core capability，只是平台 UI 对当前文本的本地查找。
- 跨章节、跨作品、索引、替换审计等能力必须下沉到 Core 后才能视为收口。

## 11. AICapability

| Capability | Core domain / facade | Core API | Android current 入口 | Android legacy 残留 | Linux current 入口 | Linux legacy 残留 | 是否对齐 | 风险级别 | 下一步 |
|---|---|---|---|---|---|---|---|---|---|
| AI 上下文/动作模型 | `ai_service` | 仅 Core 内部 facade：`build_ai_context`, `process_ai_response`; 未暴露平台稳定 `WriterCoreApi` | `SettingsActivity` 仅显示开关/可用性；无主 AI 调用链路 | 无主路径 | 缺失 | 无 | 未产品化 | P2 | 先定义 AI Context/Event/Command 的 Core API 与隐私边界 |

**小结**：
- Core 有 AI service 雏形和写作统计中的 AI 插入来源，但没有平台稳定的 AI 功能上下文入口。
- AI 能力不得在 Android/Linux UI 层自行拼上下文或读取稿件内容。

---

## 下一步重构顺序 (Refactoring Roadmap)

1. **P1-1：Linux 消费统一事件/envelope**
   - Android 的 Project/Volume/Chapter、Settings、Sync 高风险写操作已消费 Core envelope。
   - Linux 仍需在需要 UI 事件联动的保存/同步路径中消费同一 `changedEntities` / `errorCode` 语义。
2. **P1-2：缩小 legacy fallback 暴露面**
   - Android `NativeCoreBridge` 仅允许 fallback，不允许新业务接入。
   - Linux legacy facade 仅作为未迁移能力标记，新增能力必须进 `WriterCoreApi`。
3. **P1-3：MindMap 编辑入口产品化**
   - Core 已有强类型 DTO，下一步补双端主编辑/保存入口。
   - 写入口如需要状态刷新，应补标准 envelope 或等价事件标记。
4. **P1-4：Trash 完整回收站系统**
   - 当前已有 delete guard + trash + tombstone 基础。
   - 仍需补 `list_trash`、`restore_from_trash`、`purge_trash`、清理策略和 UI 权限边界。
5. **P2-1：Search / AI capability 定义**
   - Search 先定义 Core 搜索范围、索引策略、替换审计和结果 DTO。
   - AI 先定义 Core 上下文构建、隐私过滤、事件/命令模型，再允许平台接入。
